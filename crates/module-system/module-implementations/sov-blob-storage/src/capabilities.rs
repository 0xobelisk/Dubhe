use std::cmp::Ordering;

use borsh::BorshDeserialize;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::runtime::capabilities::BlobSelector;
use sov_modules_api::{
    Batch, BlobData, BlobDataWithId, BlobReaderTrait, DaSpec, KernelWorkingSet, Spec,
    StateCheckpoint,
};
use sov_sequencer_registry::AllowedSequencerError;
use tracing::{error, info, warn};

use crate::{
    BlobStorage, PreferredBlobData, PreferredBlobDataWithId, SequenceNumber, DEFERRED_SLOTS_COUNT,
    UNREGISTERED_BLOBS_PER_SLOT,
};

/// Why blob can be discarded
#[derive(Debug)]
enum BlobDiscardReason {
    /// More complicated case for preferred sequencer. Ping @prestonevans__ at Twitter for more info
    SequenceNumberTooLow,
    /// Sender doesn't have enough staked sequencer funds
    SenderInsufficientStake,
    /// The max amount of unregistered blobs allowed to be processed per slot
    MaxAllowedUnregisteredBlobs,
}

enum SequencerStatus {
    Registered,
    Unregistered,
}

enum ValidateBlobOutcome {
    Discard(BlobDiscardReason),
    Accept(SequencerStatus),
}

impl<S: Spec, Da: DaSpec> BlobStorage<S, Da> {
    /// Select the blobs to execute this slot using "based sequencing". In this mode,
    /// blobs are processed in the order that they appear on the DA layer.
    pub fn select_blobs_as_based_sequencer<'a, 'k, I>(
        &self,
        current_blobs: I,
        state: &mut KernelWorkingSet<'k, S>,
    ) -> Vec<(BlobDataWithId, Da::Address)>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        tracing::trace!("On based sequencer path");
        self.chain_state
            .set_next_visible_slot_number(&(state.current_slot().saturating_add(1)), state)
            .unwrap_infallible();
        self.select_blobs_da_ordering(current_blobs, state)
    }

    fn select_blobs_da_ordering<'a, 'k, I>(
        &self,
        current_blobs: I,
        state: &mut KernelWorkingSet<'k, S>,
    ) -> Vec<(BlobDataWithId, Da::Address)>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        let mut batches = Vec::new();
        let mut unregistered_blobs = 0;
        for (idx, blob) in current_blobs.into_iter().enumerate() {
            tracing::trace!("processing blob {}", idx);
            match self.validate_blob_and_sender(blob, unregistered_blobs, state) {
                ValidateBlobOutcome::Discard(reason) => self.log_discarded_blob(blob, reason),
                ValidateBlobOutcome::Accept(sequencer_status) => {
                    let from_registered_sequencer =
                        matches!(sequencer_status, SequencerStatus::Registered);

                    if !from_registered_sequencer {
                        unregistered_blobs += 1;
                    }
                    tracing::trace!(%from_registered_sequencer);

                    if let Some(mut data) = self.deserialize_or_try_slash_sender::<BlobData>(
                        blob,
                        from_registered_sequencer,
                        state.inner,
                    ) {
                        tracing::trace!(
                            "Successfully deserialized blob {} ({}). Adding to batch.",
                            idx,
                            hex::encode(blob.hash()),
                        );
                        if !from_registered_sequencer {
                            if let BlobData::Batch(ref mut batch) = data {
                                self.process_unregistered_batch(blob, batch);
                            }
                        }

                        batches.push((
                            BlobDataWithId {
                                data,
                                id: blob.hash(),
                                from_registered_sequencer,
                            },
                            blob.sender(),
                        ));
                    }
                }
            };
        }
        batches
    }

    fn validate_blob_and_sender(
        &self,
        blob: &Da::BlobTransaction,
        unregistered_blobs_processed: u64,
        state: &mut KernelWorkingSet<S>,
    ) -> ValidateBlobOutcome {
        match self
            .sequencer_registry
            .is_sender_allowed(&blob.sender(), state)
        {
            Ok(_) => ValidateBlobOutcome::Accept(SequencerStatus::Registered),
            Err(e) => match e {
                AllowedSequencerError::InsufficientStakeAmount { .. } => {
                    ValidateBlobOutcome::Discard(BlobDiscardReason::SenderInsufficientStake)
                }
                AllowedSequencerError::NotRegistered => {
                    if unregistered_blobs_processed >= UNREGISTERED_BLOBS_PER_SLOT {
                        ValidateBlobOutcome::Discard(BlobDiscardReason::MaxAllowedUnregisteredBlobs)
                    } else {
                        ValidateBlobOutcome::Accept(SequencerStatus::Unregistered)
                    }
                }
            },
        }
    }

    fn process_unregistered_batch(&self, blob: &Da::BlobTransaction, batch: &mut Batch) {
        let txs_count = batch.txs.len();
        if txs_count > 1 {
            batch.txs.truncate(1);
            info!(
                blob_hash = hex::encode(blob.hash()),
                sender = hex::encode(blob.sender()),
                dropped_count = txs_count - 1,
                "Dropped txs from batch, only 1 unregistered tx allowed"
            );
        }
    }

    fn log_discarded_blob(&self, blob: &Da::BlobTransaction, reason: BlobDiscardReason) {
        info!(
            blob_hash = hex::encode(blob.hash()),
            sender = hex::encode(blob.sender()),
            ?reason,
            "Discarding blob"
        );
    }

    /// Enforce the ordering constraints on preferred blobs by discarding or deferring blobs that arrive
    /// out of sequence.
    fn enforce_preferred_blob_ordering(
        &self,
        preferred_blob: PreferredBlobData,
        next_sequence_number: SequenceNumber,
        blob: &Da::BlobTransaction,
        state: &mut StateCheckpoint<S>,
    ) -> Option<PreferredBlobDataWithId> {
        match preferred_blob.sequence_number.cmp(&next_sequence_number) {
            Ordering::Equal => {
                // If the blob has the next sequence number, we'll process it.
                Some(PreferredBlobDataWithId {
                    inner: preferred_blob,
                    id: blob.hash(),
                })
            }
            Ordering::Greater => {
                // If the sequence number is greater than the expected one, we defer the blob
                let sequence_number = preferred_blob.sequence_number;
                self.deferred_preferred_sequencer_blobs
                    .set(
                        &sequence_number,
                        &PreferredBlobDataWithId {
                            inner: preferred_blob,
                            id: blob.hash(),
                        },
                        state,
                    )
                    .unwrap_infallible();
                None
            }
            Ordering::Less => {
                // If the sequence number is less than the expected one, we discard the blob
                self.log_discarded_blob(blob, BlobDiscardReason::SequenceNumberTooLow);
                None
            }
        }
    }

    /// Select blobs when transitioning from a preferred sequencer back to normal operation.
    /// This occurs when the preferred sequencer was slashed for malicious behavior. In recovery mode,
    /// the rollup processes two virtual slots at a time until it catches up to the current slot, after
    /// which it performs standard based sequencing.
    fn select_blobs_in_recovery_mode<'a, 'k, I>(
        &self,
        current_blobs: I,
        state: &mut KernelWorkingSet<'k, S>,
    ) -> Vec<(BlobDataWithId, Da::Address)>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        tracing::trace!("On recovery mode path");
        let mut batches_to_process = Vec::new();

        // First, decide how many slots worth of stored blobs we need. It could be 0, 1, or 2.
        let batches_needed_from_this_slot =
            match state.current_slot().saturating_sub(state.virtual_slot()) {
                // If the virtual slot has caught up to the current slot, we don't need any stored blobs.
                // In this case, we act like a normal "based" rollup
                0 => return self.select_blobs_as_based_sequencer(current_blobs, state),
                // If the virtual slot is only trailing by one, we process one stored slot (to catch up) and
                // then process the new blobs from this slot
                1 => {
                    self.select_blobs_as_based_sequencer(current_blobs, state);
                    1
                }
                // Otherwise, we need to process two slots from storage  - which means that we need to save the new blobs
                _ => {
                    let new_batches = self.select_blobs_da_ordering(current_blobs, state);
                    self.store_batches(state.current_slot(), &new_batches, state.inner);
                    2
                }
            };

        for slot in 0..batches_needed_from_this_slot {
            let slot_to_check = state.virtual_slot().saturating_add(slot);
            let batches_from_next_slot =
                self.take_blobs_for_slot_number(slot_to_check, state.inner);
            batches_to_process.extend(batches_from_next_slot.into_iter());
        }

        self.chain_state
            .set_next_visible_slot_number(&(state.virtual_slot().saturating_add(2)), state)
            .unwrap_infallible();

        batches_to_process
    }

    // We have two cases to handle:
    //   Step 0: Retrieve the next preferred sequencer blob in sequence (if any). Add to list of blobs to execute. Such a blob exists only in the presence of DA shenanigans
    //   Step 1: Filter any ineligible blobs from current slot. Sort by sender (preferred sequencer first)
    //   Step 2: Deserialize all preferred sequencer blobs. Stash any future sequence numbers for later retrieval.
    //   On deserialization failure, slash preferred sequencer. If preferred sequencer is slashed, enter recovery mode.
    //   Recovery mode: Start processing forced transactions two slots at a time until we’re back to real-time processing.
    //   Step 3: Find number of virtual slots to advance. Retrieve all stored (non-preferred) blobs from those slots. These become our blobs
    //   Step 4: Deserialize all blobs into batches. Return (Batch, Sender)
    /// Select the blobs to execute this slot based on the preferred sequencer and set the next virtual_height
    #[tracing::instrument(skip_all)]
    fn select_blobs_for_preferred_sequencer<'a, 'k, I>(
        &self,
        current_blobs: I,
        state: &mut KernelWorkingSet<'k, S>,
        preferred_sender: &Da::Address,
    ) -> Vec<(BlobDataWithId, Da::Address)>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        tracing::trace!("On preferred sequencer path");
        let mut unregistered_blobs = 0;
        let mut new_forced_blobs = Vec::new();
        let next_sequence_number = self
            .next_sequence_number
            .get(state)
            .unwrap_infallible()
            .unwrap_or(0);

        // Step 0: Retrieve the next preferred batch from storage, if applicable
        let mut preferred_blob = self
            .deferred_preferred_sequencer_blobs
            .remove(&next_sequence_number, state.inner)
            .unwrap_infallible();

        for (idx, blob) in current_blobs.into_iter().enumerate() {
            tracing::trace!("Checking blob {}", idx);
            match self.validate_blob_and_sender(blob, unregistered_blobs, state) {
                ValidateBlobOutcome::Discard(reason) => self.log_discarded_blob(blob, reason),
                ValidateBlobOutcome::Accept(sequencer_status) => {
                    let from_registered_sequencer =
                        matches!(sequencer_status, SequencerStatus::Registered);

                    if !from_registered_sequencer {
                        unregistered_blobs += 1;
                    }

                    // Check if the blob is from the preferred sequencer
                    if &blob.sender() == preferred_sender {
                        let maybe_batch = self
                            .deserialize_or_try_slash_sender::<PreferredBlobData>(
                                blob,
                                from_registered_sequencer,
                                state.inner,
                            )
                            .and_then(|batch| {
                                self.enforce_preferred_blob_ordering(
                                    batch,
                                    next_sequence_number,
                                    blob,
                                    state.inner,
                                )
                            });

                        // Even if we retrieved `preferred_blob`` in `step0``, we override it because it has the same `sequence_number`.
                        if let Some(next_preferred_blob) = maybe_batch {
                            preferred_blob = Some(next_preferred_blob);
                        }
                    } else {
                        // Otherwise, the batch is from a valid sender (checked in step 1) but not the preferred sender
                        // Deserialize it as a normal batch and store it in memory
                        let data = self.deserialize_or_try_slash_sender::<BlobData>(
                            blob,
                            from_registered_sequencer,
                            state.inner,
                        );
                        if let Some(mut data) = data {
                            if !from_registered_sequencer {
                                if let BlobData::Batch(ref mut batch) = data {
                                    self.process_unregistered_batch(blob, batch);
                                }
                            }

                            new_forced_blobs.push((
                                BlobDataWithId {
                                    data,
                                    id: blob.hash(),
                                    from_registered_sequencer,
                                },
                                blob.sender(),
                            ));
                        }
                    }
                }
            }
        }

        // Step 3: Find number of virtual slots to advance.
        // - If the preferred sequencer requested a number, advance up to that many (stopping early if the next virtual slot would be in the future)
        // - Otherwise, advance only if we would otherwise exceed the maximum deferred slots count
        let max_slots_to_advance = state
            .current_slot()
            .saturating_sub(state.virtual_slot())
            .saturating_add(1);
        let mut batches_to_process = Vec::new();

        let num_slots_to_advance = if let Some(preferred_blob) = preferred_blob {
            self.next_sequence_number
                .set(&next_sequence_number.saturating_add(1), state)
                .unwrap_infallible();

            let first_batch = BlobDataWithId {
                data: preferred_blob.inner.data,
                id: preferred_blob.id,
                // This is a preferred blob so it is from the preferred sequencer
                // hence the sequencer is a registered one
                from_registered_sequencer: true,
            };

            batches_to_process.push((first_batch, preferred_sender.clone()));
            tracing::debug!(
                seq_number = preferred_blob.inner.sequence_number,
                slots_to_advance = preferred_blob.inner.virtual_slots_to_advance,
                "Requested to advance slots"
            );

            if preferred_blob.inner.virtual_slots_to_advance as u64 > max_slots_to_advance {
                warn!(
                    "Preferred sequencer requested {} slots, but we can only advance {} slots",
                    preferred_blob.inner.virtual_slots_to_advance, max_slots_to_advance
                );
                max_slots_to_advance
            } else {
                std::cmp::max(preferred_blob.inner.virtual_slots_to_advance as u64, 1)
            }
        } else {
            // If there's no preferred blob, advance only if the we would otherwise exceed the maximum deferred slots count
            if state.virtual_slot().saturating_add(DEFERRED_SLOTS_COUNT) <= state.current_slot() {
                1
            } else {
                0
            }
        };
        tracing::debug!(
            num_slots_to_advance,
            current_real_slot = state.current_slot(),
            "Advancing virtual slot number"
        );

        // Load all the necessary batches from storage
        for slot in 0..num_slots_to_advance {
            let slot_to_check = state.virtual_slot().saturating_add(slot);
            let batches_from_next_slot =
                self.take_blobs_for_slot_number(slot_to_check, state.inner);
            tracing::trace!(
                "Found {} additional blobs in slot {} ",
                batches_from_next_slot.len(),
                slot_to_check
            );
            batches_to_process.extend(batches_from_next_slot.into_iter());
        }

        // Check if we also need the blobs from the current slot. Add them to the set to be processed or store them as appropriate.
        let next_virtual_height = state.virtual_slot().saturating_add(num_slots_to_advance);
        if next_virtual_height > state.current_slot() {
            batches_to_process.extend(new_forced_blobs);
        } else {
            self.store_batches(state.current_slot(), &new_forced_blobs, state.inner);
        }
        self.chain_state
            .set_next_visible_slot_number(&next_virtual_height, state)
            .unwrap_infallible();
        batches_to_process
    }

    /// Deserialize a blob into a `Batch` or slash the sender if it's malformed.
    /// The sequencer might not exist if we're processing a blob submitted by an unregistered
    /// sequencer - in the case of direct sequencer registration via DA.
    fn deserialize_or_try_slash_sender<B: BorshDeserialize>(
        &self,
        blob: &mut Da::BlobTransaction,
        registered_sender: bool,
        state: &mut StateCheckpoint<S>,
    ) -> Option<B> {
        match B::try_from_slice(data_for_deserialization(blob)) {
            Ok(batch) => Some(batch),
            // if the blob is malformed, slash the sequencer
            Err(e) => {
                assert_eq!(blob.verified_data().len(), blob.total_len(), "Batch deserialization failed and some data was not provided. The prover might be malicious");
                error!(
                    blob_hash = hex::encode(blob.hash()),
                    slashed_sender = %blob.sender(),
                    error = ?e,
                    "Unable to deserialize blob. slashing sender if they are registered"
                );

                if registered_sender {
                    self.sequencer_registry
                        .slash_sequencer(&blob.sender(), state);
                } else {
                    info!("Unable to slash sequencer, they were not registered");
                }

                None
            }
        }
    }
}

impl<S: Spec, Da: DaSpec> BlobSelector<Da> for BlobStorage<S, Da> {
    type Spec = S;

    type BlobType = BlobDataWithId;

    // This implementation returns three categories of blobs:
    // 1. Any blobs sent by the preferred sequencer ("prority blobs")
    // 2. Any non-priority blobs which were sent `DEFERRED_SLOTS_COUNT` slots ago ("expiring deferred blobs")
    // 3. Some additional deferred blobs needed to fill the total requested by the sequencer, if applicable. ("bonus blobs")
    fn get_blobs_for_this_slot<'a, 'k, I>(
        &self,
        current_blobs: I,
        state: &mut KernelWorkingSet<'k, S>,
    ) -> anyhow::Result<Vec<(Self::BlobType, Da::Address)>>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        // If `DEFERRED_SLOTS_COUNT` is 0, we treat the rollup as having no preferred sequencer.
        // In this case, we just process blobs in the order that they appeared on the DA layer
        if DEFERRED_SLOTS_COUNT == 0 {
            return Ok(self.select_blobs_as_based_sequencer(current_blobs, state));
        }

        // If there's a preferred sequencer, sequence accordingly.
        if let Some(preferred_sender) = self.get_preferred_sequencer(state.inner) {
            return Ok(self.select_blobs_for_preferred_sequencer(
                current_blobs,
                state,
                &preferred_sender,
            ));
        }

        // Otherwise, we're configured for a preferred sequencer but one doesn't exist. This usually means that the preferred sequencer was slashed.
        // Entery recovery mode.
        Ok(self.select_blobs_in_recovery_mode(current_blobs, state))
    }
}

#[cfg(feature = "native")]
fn data_for_deserialization(blob: &mut impl BlobReaderTrait) -> &[u8] {
    blob.full_data()
}

#[cfg(not(feature = "native"))]
fn data_for_deserialization(blob: &mut impl BlobReaderTrait) -> &[u8] {
    blob.verified_data()
}

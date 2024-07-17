// use borsh::{BorshDeserialize, BorshSerialize};
use celestia_types::nmt::NamespaceProof;
use serde::{Deserialize, Serialize};

use super::CelestiaSpec;
use crate::types::{NamespaceData, Row};
use crate::CelestiaHeader;

// TODO: derive borsh Serialize, Deserialize <https://github.com/eigerco/celestia-node-rs/issues/155>
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct EtxProof {
    pub proof: Vec<EtxRangeProof>,
}

// TODO: derive borsh Serialize, Deserialize <https://github.com/eigerco/celestia-node-rs/issues/155>
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct EtxRangeProof {
    pub shares: Vec<Vec<u8>>,
    pub proof: NamespaceProof,
    pub start_share_idx: usize,
    pub start_offset: usize,
}

pub fn new_inclusion_proof(
    header: &CelestiaHeader,
    pfb_rows: &[Row],
    rollup_data: &NamespaceData,
    blobs: &[<CelestiaSpec as sov_rollup_interface::da::DaSpec>::BlobTransaction],
) -> Vec<EtxProof> {
    let mut needed_tx_shares = Vec::new();

    // Extract (and clone) the position of each transaction
    for tx in blobs.iter() {
        let (_, position) = rollup_data
            .relevant_pfbs
            .get(tx.hash.0.as_slice())
            .expect("commitment must exist in map");
        needed_tx_shares.push(position.clone());
    }

    let mut needed_tx_shares = needed_tx_shares.into_iter().peekable();
    let mut current_tx_proof: EtxProof = EtxProof { proof: Vec::new() };
    let mut tx_proofs: Vec<EtxProof> = Vec::with_capacity(blobs.len());

    for (row_idx, row) in pfb_rows.iter().enumerate() {
        let mut nmt = row.merklized();
        while let Some(next_needed_share) = needed_tx_shares.peek_mut() {
            // If the next needed share falls in this row
            let row_start_idx = header
                .square_size()
                .checked_mul(row_idx)
                .expect("invalid row");
            let start_column_number = next_needed_share
                .share_range
                .start
                .checked_sub(row_start_idx)
                .expect("invalid row");
            if start_column_number < header.square_size() {
                let end_column_number = next_needed_share
                    .share_range
                    .end
                    .checked_sub(row_start_idx)
                    .expect("invalid row");
                if end_column_number <= header.square_size() {
                    let (shares, proof) =
                        nmt.get_range_with_proof(start_column_number..end_column_number);

                    current_tx_proof.proof.push(EtxRangeProof {
                        shares,
                        proof: proof.into(),
                        start_offset: next_needed_share.start_offset,
                        start_share_idx: next_needed_share.share_range.start,
                    });
                    tx_proofs.push(current_tx_proof);
                    current_tx_proof = EtxProof { proof: Vec::new() };
                    let _ = needed_tx_shares.next();
                } else {
                    let (shares, proof) =
                        nmt.get_range_with_proof(start_column_number..header.square_size());

                    current_tx_proof.proof.push(EtxRangeProof {
                        shares,
                        proof: proof.into(),
                        start_offset: next_needed_share.start_offset,
                        start_share_idx: next_needed_share.share_range.start,
                    });
                    next_needed_share.share_range.start = row_idx
                        .checked_add(1)
                        .expect("invalid row id")
                        .checked_mul(header.square_size())
                        .expect("invalid square size");

                    next_needed_share.start_offset = 0;

                    break;
                }
            } else {
                break;
            }
        }
    }
    tx_proofs
}

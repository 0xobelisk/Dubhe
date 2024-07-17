use borsh::{BorshDeserialize, BorshSerialize};
use celestia_types::nmt::{Namespace, NS_SIZE};
use celestia_types::NamespacedShares;
use prost::bytes::Buf;
use serde::{Deserialize, Serialize};
use sov_rollup_interface::Bytes;

use crate::utils::read_varint;
use crate::verifier::PFB_NAMESPACE;

/// The length of the "reserved bytes" field in a compact share
pub const RESERVED_BYTES_LEN: usize =
    celestia_types::consts::appconsts::COMPACT_SHARE_RESERVED_BYTES;
/// The length of the "info byte" field in a compact share
pub const INFO_BYTE_LEN: usize = celestia_types::consts::appconsts::SHARE_INFO_BYTES;
/// The length of the "sequence length" field
pub const SEQUENCE_LENGTH_BYTES: usize = celestia_types::consts::appconsts::SEQUENCE_LEN_BYTES;
/// The size of a share, in bytes
pub const SHARE_SIZE: usize = celestia_types::consts::appconsts::SHARE_SIZE;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
pub enum Share {
    Continuation(Bytes),
    Start(Bytes),
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum ShareError {
    NotAStartShare,
    InvalidEncoding,
}

impl Share {
    pub fn new(inner: Bytes) -> Self {
        enforce_version_zero(inner.as_ref());
        if is_continuation_unchecked(inner.as_ref()) {
            Self::Continuation(inner)
        } else {
            Self::Start(inner)
        }
    }

    pub fn from_slice(slice: impl AsRef<[u8]>) -> Self {
        Self::new(Bytes::copy_from_slice(slice.as_ref()))
    }

    pub fn as_serialized(&self) -> &[u8] {
        self.raw_inner_ref()
    }

    pub fn is_sequence_start(&self) -> bool {
        match self {
            Share::Continuation(_) => false,
            Share::Start(_) => true,
        }
    }

    pub fn sequence_length(&self) -> Result<u64, ShareError> {
        match self {
            Share::Continuation(_) => Err(ShareError::NotAStartShare),
            Share::Start(inner) => {
                let mut inner = inner.clone();
                inner.advance(NS_SIZE + INFO_BYTE_LEN);
                Ok(inner.get_u32() as u64)
            }
        }
    }

    /// Returns this share in raw serialized form as Bytes
    fn raw_inner(&self) -> Bytes {
        match self {
            Share::Continuation(inner) | Share::Start(inner) => inner.clone(),
        }
    }

    /// Returns this share in raw serialized form as a slice
    fn raw_inner_ref(&self) -> &[u8] {
        match self {
            Share::Continuation(inner) | Share::Start(inner) => inner.as_ref(),
        }
    }

    pub fn hash(&self) -> [u8; 32] {
        use sha2::Digest;
        sha2::Sha256::digest(self.raw_inner_ref()).into()
    }

    /// Returns the offset *into the data portion* of this share at which
    /// the first tx begins. So, for example, if this share is the start of a sequence,
    /// the returned offset will be 0.
    fn offset_of_first_tx(&self) -> Option<usize> {
        // FIXME: account for continuation vs. start shares
        match self {
            Share::Continuation(_) => {
                let reserved_bytes_offset = NS_SIZE + INFO_BYTE_LEN;
                let mut raw = self.raw_inner();
                raw.advance(reserved_bytes_offset);
                let idx_of_next_start = raw.get_u32() as usize;
                if idx_of_next_start == 0 {
                    None
                } else {
                    Some(idx_of_next_start - self.get_data_offset())
                }
            }
            // Start shares always have a sequence beginning at the first byte
            Share::Start(_) => Some(0),
        }
    }

    fn get_data_offset(&self) -> usize {
        // All shares are prefixed with metadata including the namespace (8 bytes), and info byte (1 byte)
        let mut offset = NS_SIZE + INFO_BYTE_LEN;
        // Start shares are also prefixed with a sequence length
        if let Self::Start(_) = self {
            offset += SEQUENCE_LENGTH_BYTES;
        }
        if self.namespace().is_reserved_on_celestia() {
            // Compact shares (shares in reserved namespaces) are prefixed with 4 reserved bytes
            offset += RESERVED_BYTES_LEN;
        }
        offset
    }

    /// Returns the data of this share as a `Bytes`
    pub fn data(&self) -> Bytes {
        let mut output = self.raw_inner();
        output.advance(self.get_data_offset());
        output
    }

    /// Returns the data of this share as &[u8]
    pub fn data_ref(&self) -> &[u8] {
        &self.raw_inner_ref()[self.get_data_offset()..]
    }

    /// Get the namespace associated with this share
    pub fn namespace(&self) -> Namespace {
        let out: [_; NS_SIZE] = self.raw_inner_ref()[..NS_SIZE]
            .try_into()
            .expect("can't fail for correct size");
        nmt_rs::NamespaceId(out).into()
    }

    pub fn is_valid_tx_start(&self, idx: usize) -> bool {
        if self.namespace() != PFB_NAMESPACE {
            return false;
        }
        // Check if this share contains the start of any txs
        if let Some(mut next_legal_start_offset) = self.offset_of_first_tx() {
            let mut remaining_data = self.data();
            // If so, iterate over the txs in this share and check if the given idx is the start of one of them
            loop {
                if next_legal_start_offset == idx {
                    return true;
                }
                if let Ok((tx_len, len_of_len)) = read_varint(&mut remaining_data) {
                    next_legal_start_offset += tx_len as usize + len_of_len;
                    remaining_data.advance(tx_len as usize);
                } else {
                    return false;
                }
            }
        }
        // Otherwise, the start index is invalid
        false
    }
}

impl AsRef<[u8]> for Share {
    fn as_ref(&self) -> &[u8] {
        match self {
            Share::Continuation(c) => c.as_ref(),
            Share::Start(s) => s.as_ref(),
        }
    }
}

fn is_continuation_unchecked(share: &[u8]) -> bool {
    share[NS_SIZE] & 0x01 == 0
}

fn enforce_version_zero(share: &[u8]) {
    assert_eq!(share[NS_SIZE] & !0x01, 0);
}

/// A group of shares, in a single namespace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
pub enum NamespaceGroup {
    Compact(Vec<Share>),
    Sparse(Vec<Share>),
}

impl NamespaceGroup {
    pub fn from_shares<I, B>(shares: I) -> Self
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        shares
            .into_iter()
            .map(|bytes| Share::new(Bytes::copy_from_slice(bytes.as_ref())))
            .collect()
    }

    pub fn shares(&self) -> &Vec<Share> {
        match self {
            NamespaceGroup::Compact(shares) | NamespaceGroup::Sparse(shares) => shares,
        }
    }

    pub fn blobs(&self) -> NamespaceIterator {
        NamespaceIterator {
            offset: 0,
            shares: self,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.shares().is_empty()
    }
}

impl FromIterator<Share> for NamespaceGroup {
    fn from_iter<T: IntoIterator<Item = Share>>(iter: T) -> Self {
        let shares: Vec<_> = iter.into_iter().collect();
        if shares.is_empty() {
            // if there are no shares at all return sparse empty group
            return NamespaceGroup::Sparse(vec![]);
        }

        if shares[0].namespace().is_reserved_on_celestia() {
            NamespaceGroup::Compact(shares)
        } else {
            NamespaceGroup::Sparse(shares)
        }
    }
}

impl From<&NamespacedShares> for NamespaceGroup {
    fn from(value: &NamespacedShares) -> Self {
        Self::from_shares(value.rows.iter().flat_map(|row| row.shares.iter()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
pub struct Blob(pub Vec<Share>);

impl<'a> From<BlobRef<'a>> for Blob {
    fn from(value: BlobRef<'a>) -> Self {
        Self(value.0.to_vec())
    }
}

impl IntoIterator for Blob {
    type Item = u8;

    type IntoIter = BlobIterator;

    fn into_iter(self) -> Self::IntoIter {
        let sequence_length = self.0[0]
            .sequence_length()
            .expect("blob must contain start share at idx 0");
        BlobIterator {
            sequence_len: sequence_length as usize,
            consumed: 0,
            current: self.0[0].data(),
            current_idx: 0,
            blob: self,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlobRef<'a>(pub &'a [Share]);

impl<'a> BlobRef<'a> {
    pub fn with(shares: &'a [Share]) -> Self {
        Self(shares)
    }

    pub fn data(&self) -> BlobRefIterator {
        let sequence_length = self.0[0]
            .sequence_length()
            .expect("blob must contain start share at idx 0");
        BlobRefIterator {
            sequence_len: sequence_length as usize,
            consumed: 0,
            current: self.0[0].data(),
            current_idx: 0,
            shares: self.0,
        }
    }

    pub fn is_padding(&self) -> bool {
        // Padding checks as per (https://github.com/celestiaorg/celestia-app/blob/main/specs/src/specs/shares.md#padding)
        // 1. Start share
        // 2. Sequence length is 0
        // 3. Remaining non metadata bytes are 0
        // The sequence_length check also checks for start share.
        // Zero check is not strictly necessary but is added to be as specific as possible
        self.0[0]
            .sequence_length()
            .expect("blob must contain start share at idx 0")
            == 0
            && self.0[0].data().iter().all(|&byte| byte == 0)
    }

    pub fn celestia_shares(&self) -> Vec<celestia_types::Share> {
        self.0
            .iter()
            .map(|s| celestia_types::Share::from_raw(s.as_ref()).expect("Cannot convert share"))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, BorshDeserialize, BorshSerialize, PartialEq)]
pub struct BlobIterator {
    sequence_len: usize,
    consumed: usize,
    current: Bytes,
    current_idx: usize,
    blob: Blob,
}

impl Iterator for BlobIterator {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.consumed == self.sequence_len {
            return None;
        }
        if self.current.has_remaining() {
            self.consumed += 1;
            return Some(self.current.get_u8());
        }
        self.current_idx += 1;
        self.current = self.blob.0[self.current_idx].data();
        self.next()
    }
}

impl std::io::Read for BlobIterator {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut written = 0;
        for byte in buf.iter_mut() {
            *byte = match self.next() {
                Some(byte) => {
                    written += 1;
                    byte
                }
                None => return Ok(written),
            };
        }
        Ok(written)
    }
}

impl Buf for BlobIterator {
    fn remaining(&self) -> usize {
        self.sequence_len
            .checked_sub(self.consumed)
            .expect("BlobIterator has consumed more than available bytes")
    }

    fn chunk(&self) -> &[u8] {
        let chunk = if self.current.has_remaining() {
            self.current.as_ref()
        } else {
            // If the current share is exhausted, try to take the data from the next one
            // if there is no next chunk, we're done. Return the empty slice.
            if self.current_idx + 1 >= self.blob.0.len() {
                return &[];
            }
            // Otherwise, take the next chunk
            self.blob.0[self.current_idx + 1].data_ref()
        };
        // Chunks are zero-padded, so truncate if necessary
        let remaining = self.remaining();
        if chunk.len() > remaining {
            return &chunk[..remaining];
        }
        chunk
    }

    fn advance(&mut self, mut cnt: usize) {
        self.consumed += cnt;
        while cnt > 0 {
            let remaining_in_current_share = self.current.remaining();
            if remaining_in_current_share > cnt {
                self.current.advance(cnt);
                return;
            } else {
                // Exhaust the current share
                self.current.advance(remaining_in_current_share);
                // If possible, advance the current share idx so that any future calls to `chunk` return
                // a non-empty array.
                if (self.current_idx + 1) < self.blob.0.len() {
                    self.current_idx += 1;
                    self.current = self.blob.0[self.current_idx].data();
                } else {
                    // If advancing the current share was not possible, then we must have exactly used up the bytes
                    // in this blob. Assert that this is the case
                    assert_eq!(
                        remaining_in_current_share, cnt,
                        "Exhausted last share without fulfilling request!"
                    );
                }
                cnt -= remaining_in_current_share;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlobRefIterator<'a> {
    sequence_len: usize,
    consumed: usize,
    current: Bytes,
    current_idx: usize,
    shares: &'a [Share],
}

impl<'a> BlobRefIterator<'a> {
    pub fn current_position(&self) -> (usize, usize) {
        (
            self.current_idx,
            self.shares[self.current_idx].data_ref().len() - self.current.remaining(),
        )
    }
}

impl<'a> Iterator for BlobRefIterator<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.consumed == self.sequence_len {
            return None;
        }
        if self.current.has_remaining() {
            self.consumed += 1;
            return Some(self.current.get_u8());
        }
        self.current_idx += 1;
        self.current = self.shares[self.current_idx].data();
        self.next()
    }
}

impl<'a> Buf for BlobRefIterator<'a> {
    fn remaining(&self) -> usize {
        self.sequence_len - self.consumed
    }

    fn chunk(&self) -> &[u8] {
        let chunk = if self.current.has_remaining() {
            self.current.as_ref()
        } else {
            // If the current share is exhausted, try to take the data from the next one
            // if there is no next chunk, we're done. Return the empty slice.
            if self.current_idx + 1 >= self.shares.len() {
                return &[];
            }
            // Otherwise, take the next chunk
            self.shares[self.current_idx + 1].data_ref()
        };
        // Chunks are zero-padded, so truncate if necessary
        let remaining = self.remaining();
        if chunk.len() > remaining {
            return &chunk[..remaining];
        }
        chunk
    }

    fn advance(&mut self, mut cnt: usize) {
        self.consumed += cnt;
        while cnt > 0 {
            if self.current.remaining() >= cnt {
                self.current.advance(cnt);
                return;
            }

            cnt = cnt
                .checked_sub(self.current.remaining())
                .expect("Underflow!");

            self.current.advance(self.current.remaining());
            self.current_idx += 1;
            match self.shares.get(self.current_idx) {
                Some(share) => self.current = share.data(),
                None => assert_eq!(cnt, 0, "Tried to read more bytes than are available"),
            }
        }
    }
}

pub struct NamespaceIterator<'a> {
    offset: usize,
    shares: &'a NamespaceGroup,
}

impl<'a> Iterator for NamespaceIterator<'a> {
    type Item = BlobRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.shares.shares().len() {
            return None;
        }
        match self.shares {
            NamespaceGroup::Compact(shares) => {
                self.offset = shares.len();
                Some(BlobRef::with(shares))
            }
            NamespaceGroup::Sparse(shares) => {
                let start = self.offset;
                self.offset += 1;

                if self.offset == shares.len() {
                    return Some(BlobRef::with(&shares[start..self.offset]));
                }

                for (idx, share) in shares[self.offset..].iter().enumerate() {
                    if share.is_sequence_start() {
                        self.offset += idx;
                        return Some(BlobRef::with(&shares[start..self.offset]));
                    }
                }

                self.offset = shares.len();
                return Some(BlobRef::with(&shares[start..self.offset]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use celestia_types::nmt::NS_ID_V0_SIZE;
    use postcard::{from_bytes, to_allocvec, Result};
    use proptest::collection::vec;
    use proptest::prelude::*;
    use sov_rollup_interface::da::CountedBufReader;

    use super::*;

    #[test]
    fn test_share_start_serialization() {
        let hex_blob = "736f762d7465737401000000b801000000b000000004ee8ca2c343fe0acd2b72249c48b56351ebfb4b7eef73ddae363880b61380cc23b3ebf15375aa110d7aa84206b1f22c1885b26e980d5e03244cc588e314b004a60b594d5751dc2a326c18923eaa74b48424c0f246733c6c028d7ee16899ad944400000001000b000000000000000e000000736f762d746573742d746f6b656e8813000000000000a3201954f70ad62230dc3d840a5bf767702c04869e85ab3eee0b962857ba75980000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
        let sample = hex::decode(hex_blob).unwrap();
        // let share = Share::new(Bytes::from(sample.clone()));
        let share = Share::Start(Bytes::from(sample));

        let raw_share: Vec<u8> = to_allocvec(&share).unwrap();

        let decoded_share: Result<Share> = from_bytes(&raw_share);
        assert!(decoded_share.is_ok());
        let decoded_share = decoded_share.unwrap();
        assert_eq!(share, decoded_share);
    }

    #[test]
    fn test_padding_blob() {
        // Padding share
        let padding_share_hex = "00000000000000000000000000000000000000736f762d726f6c6c3035010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
        let padding_share = [Share::Start(Bytes::from(
            hex::decode(padding_share_hex).unwrap(),
        ))];
        let padding_blob = BlobRef(&padding_share);
        assert!(padding_blob.is_padding());
    }

    #[test]
    fn test_non_padding_blob() {
        // Padding share
        let share_hex = "00000000000000000000000000000000000000736f762d726f6c6c3035006db170157acaac31df9bba6cd40e7329e608eabd000000000000000000000000000000000000000000000000003326000000000000cf000000f5fc0b24032bf0b1d2f6074c4adda7b885606fc3781fca2a6bc3731258d596544ffdcd85ad6b4dd3dec3e4ad75ecbfb81c16cd28beca51ddad55c4ae3a354a0a7b758bf2e7670fafaf6bf0015ce0ff5aa802306fc7e3f45762853ffc37180fe64a000000010175fdcede95433da8366f1101b9b62519fe9f5438c8e7d03645bdec9ab669371d0100000000000000135d23aee8cb15c890831ff36db170157acaac31df9bba6cd40e7329e608eabd000000000000000000000000000000000000000000000000003426000000000000cf00000020b12d2405dc2660e67ca004839ba50fe2ee09effdae3abd5caca862d572750ea4149e49f8a2fd7c519a7dd5c4030b21f17a88c4d23ee6d14fd2e6136d8718077b758bf2e7670fafaf6bf0015ce0ff5aa802306fc7e3f45762853ffc37180fe64a00000001019f2c2726ebcc5912a95ed4d2b4d2d56a7b159d7968fa6ebca1913db3d149d2f10100000000000000135d23aee8cb15c890831ff36db170157acaac31df9bba6cd40e7329e608eabd000000000000000000000000000000000000000000000000003526000000000000cf000000cc786d";
        let share = [Share::Start(Bytes::from(hex::decode(share_hex).unwrap()))];
        let non_padding_blob = BlobRef(&share);
        assert!(!non_padding_blob.is_padding());
    }

    /// This test detects a regression where we panic on trying to read the entire contents a blob using the `Buf` trait.
    /// A previous implementation caused buf.advance(buf.remaining()) to panic, because an internal state change of
    /// `current_share = blob.shares[blob.current_idx+1]` would be triggered  as soon as the
    /// current share's data was read - even if the next share did not exist and would never be read.
    #[test]
    fn test_reading_full_blob_regression() {
        let data: NamespacedShares = serde_json::from_str("[{\"shares\":[\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAc292LXRlc3QBAAAB3gEAAADWAQAA2EYzlAhN9KIdK3QP5WgAxmNMAiO3KOZd54PG1ndFjNud8Ij/Tn3TfZMkgVVIPduDBwKbg/S5/ShVT2cNHhn7CvitJDeieeHIkywHNYyR3E/jSGSpjGwl8pjioBmcFQn/UQEAAAIBSwEAAEZCB10foCn5JspyMO6m5QA8qYnV44fWffQU2E57kHzs4hKqnEjNwYnUJABmr6B1OJ/5+v7+BgPeOpxnaK4M5XovSUs8ExC7QtzrZxZT08DiU8nKCzK0VEUcDiu72Iyhnycjfxep9Nk7NuWt18/cz7YyUWYzlQcn6AN12JxkNF/Afx+DJzDJEzFE7bpzdjvFrl0im+L+wFF/T0IWonUXPL+oCvKoNDrDr+xA25mn9/rMDG/UGT0FI+KXHqx7c3ZR+5lLQheaxdVMRQcz2Mnst9uh8JKxVVVl9Fg4d7Jh6n2f9ZbBAfwg3YhW/5szA8YPKNg6o6nVnb90Cgk2RFVSOvxym4BijKId47SK6dfCS7G69iRFrDU+OcQTxsvkNb3mmnYa4TI9TGRHwl2N1yMHoJcWIboQPcLsKuykcJV+w4bvuB+uu8/pMVPjIDYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"],\"proof\":{\"start\":1,\"end\":2,\"nodes\":[\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABH1fFJqiHwbfG4vQXdK1Oh9vrkjlqWB365SYrsZxmaER\",\"/////////////////////////////////////////////////////////////////////////////yu8ipB5WivrLzIkx/iKuwyXvQFePPFTx9ZGUEhlxazq\"],\"leaf_hash\":\"\",\"is_max_namespace_ignored\":true}}]").unwrap();
        let ns_group: NamespaceGroup = (&data).into();
        let blob: Blob = ns_group.blobs().next().unwrap().into();
        let mut blob = CountedBufReader::new(blob.into_iter());
        // Try to read the entire contents of the blob
        blob.advance(blob.total_len());
    }

    prop_compose! {
        fn share_bytes_strategy()(
            ns in vec(0u8.., NS_ID_V0_SIZE),
            mut share in vec(0u8.., SHARE_SIZE),
        ) -> Vec<u8> {
            let namespace = Namespace::new_v0(&ns).expect("doesn't exceed size");
            // overwrite namespace
            share[..NS_SIZE].copy_from_slice(namespace.as_ref());
            // set version to zero
            share[NS_SIZE] &= 0x01;

            share
        }
    }

    proptest! {
        #[test]
        fn proptest_share_serialization_deserialization(data in share_bytes_strategy()) {
            let bytes = Bytes::from(data);
            let share = Share::new(bytes);
            let serialized = borsh::to_vec(&share).unwrap();
            let deserialized: Share = Share::try_from_slice(&serialized).unwrap();
            prop_assert_eq!(share, deserialized);
        }
    }
}

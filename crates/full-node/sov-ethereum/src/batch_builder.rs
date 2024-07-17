use std::collections::VecDeque;

pub struct EthBatchBuilder {
    mempool: VecDeque<Vec<u8>>,
    min_blob_size: Option<usize>,
}

impl EthBatchBuilder {
    /// Creates a new `EthBatchBuilder`.
    pub fn new(min_blob_size: Option<usize>) -> Self {
        EthBatchBuilder {
            mempool: VecDeque::new(),
            min_blob_size,
        }
    }

    /// Signs messages with the private key of the `EthBatchBuilder` and make them `transactions`.
    /// Returns the blob of signed transactions.
    fn make_blob(&mut self) -> Vec<Vec<u8>> {
        let mut txs = Vec::new();

        while let Some(raw_message) = self.mempool.pop_front() {
            txs.push(raw_message);
        }
        txs
    }

    /// Adds `messages` to the mempool.
    pub fn add_messages(&mut self, messages: Vec<Vec<u8>>) {
        for message in messages {
            self.mempool.push_back(message);
        }
    }

    /// Attempts to create a blob with a minimum size of `min_blob_size`.
    pub fn get_next_blob(&mut self, min_blob_size: Option<usize>) -> Vec<Vec<u8>> {
        let min_blob_size = min_blob_size.or(self.min_blob_size);

        if let Some(min_blob_size) = min_blob_size {
            if self.mempool.len() >= min_blob_size {
                return self.make_blob();
            }
        }
        Vec::default()
    }
}

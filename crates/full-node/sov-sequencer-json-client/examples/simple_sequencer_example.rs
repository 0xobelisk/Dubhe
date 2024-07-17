use base64::prelude::*;
use sov_sequencer_json_client::types::AcceptTxBody;

#[tokio::main]
async fn main() {
    let client = sov_sequencer_json_client::Client::new("http://example.com");

    let response = client
        .accept_tx(&AcceptTxBody {
            body: BASE64_STANDARD.encode("I'm a transaction body!"),
        })
        .await
        .unwrap();
    println!("Response: {:?}", response);
}

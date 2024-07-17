use sov_ledger_json_client::Client;

#[tokio::main]
async fn main() {
    let client = Client::new("http://example.com");

    let latest_slot_response = client.get_latest_slot(None).await.unwrap();
    println!("Latest slot: {:?}", latest_slot_response);
}

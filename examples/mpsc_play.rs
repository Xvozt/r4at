use std::time::Duration;

#[tokio::main]
async fn main() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move {
        for i in 0..15 {
            println!("Trying to send: {i}");

            if let Err(e) = tx.send(i).await {
                println!("Reciever dropped: {e}");
                return;
            }
            println!("Successfully sent: {i}");
        }
    });

    while let Some(i) = rx.recv().await {
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("Recieved: {i}");
    }
}

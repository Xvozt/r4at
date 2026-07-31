use std::time::Duration;

use tokio::sync::mpsc::Sender;

#[tokio::main]
async fn main() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let tx_two = tx.clone();
    let tx_three = tx.clone();

    tokio::spawn(async move {
        for i in 0..5 {
            println!("Trying to send: {i}");

            if let Err(e) = tx.send(i).await {
                println!("Reciever dropped: {e}");
                return;
            }
            println!("Successfully sent: {i}");
        }
    });

    tokio::spawn(counter_job(100..105, tx_two));
    tokio::spawn(counter_job(200..205, tx_three));

    while let Some(i) = rx.recv().await {
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("Recieved: {i}");
    }
    println!("Consumer saw channel close, should be seen only once");
}

async fn counter_job(range: std::ops::Range<u32>, sender: Sender<u32>) {
    for i in range {
        println!("Trying to send: {i}");

        if let Err(e) = sender.send(i).await {
            println!("Reciever dropped: {e}");
            return;
        }
        println!("Successfully sent: {i}");
    }
}

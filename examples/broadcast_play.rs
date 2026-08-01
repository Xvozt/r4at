use std::time::Duration;

use tokio::{
    sync::broadcast::{self, error::RecvError},
    task,
};

#[tokio::main]
async fn main() {
    let (tx, rx1) = broadcast::channel(4);
    let rx2 = tx.subscribe();
    let rx3 = tx.subscribe();

    let first = task::spawn(rx_wrapper(0, 1, rx1));
    let second = task::spawn(rx_wrapper(500, 2, rx2));
    let third = task::spawn(rx_wrapper(100, 3, rx3));

    for i in 0..20 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        tx.send(i).unwrap();
    }
    drop(tx);
    let _ = tokio::join!(first, second, third);
}

async fn rx_wrapper(millis: u64, id: u32, mut rx: broadcast::Receiver<i32>) {
    loop {
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
        match rx.recv().await {
            Ok(i) => println!("Recieved for {id}: {i}"),
            Err(RecvError::Lagged(n)) => println!("Reciever: {id} lagged, lost {n}"),
            Err(RecvError::Closed) => break,
        }
    }
}

use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let now = Instant::now();
    do_job(1, 1000).await;
    do_job(2, 1000).await;
    let spent_sequentially = now.elapsed();
    println!(
        "Spent sequentially: {} secs\n",
        spent_sequentially.as_secs()
    );

    let now = Instant::now();
    tokio::join!(do_job(3, 1000), do_job(4, 1000));
    let spent_concurrently_one_task = now.elapsed();
    println!(
        "Spent concurrently: {} secs\n",
        spent_concurrently_one_task.as_secs()
    );

    let now = Instant::now();

    let first = tokio::spawn(do_job(5, 1000));
    let second = tokio::spawn(do_job(6, 1000));

    let _ = first.await;
    let _ = second.await;

    let spent_using_spawn = now.elapsed();
    println!(
        "Spent with independent tasks: {} secs\n",
        spent_using_spawn.as_secs()
    );
}

async fn do_job(id: u32, ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    println!("Done: {id}");
}

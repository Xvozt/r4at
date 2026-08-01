use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::task;

#[tokio::main]
async fn main() {
    let counter = Arc::new(Mutex::new(0));
    let mut vec_of_tasks = Vec::new();

    for _ in 0..5 {
        let counter = counter.clone();
        let task = task::spawn(async move {
            raise_count(counter);
        });
        vec_of_tasks.push(task);
    }

    let counter_for_async = counter.clone();
    let last_task = task::spawn(raise_count_async(counter_for_async, 500));

    vec_of_tasks.push(last_task);

    for t in vec_of_tasks {
        t.await.unwrap();
    }

    let result_guard = counter.lock().unwrap();
    println!("Counter: {}", *result_guard);
    drop(result_guard);
}

fn raise_count(counter: Arc<Mutex<u32>>) {
    for _ in 0..1000 {
        let mut number = counter.lock().unwrap();
        *number += 1;
    }
}

async fn raise_count_async(mutex: Arc<Mutex<u32>>, hold_time: u64) {
    {
        let mut guard = mutex.lock().unwrap();
        *guard += 1;
    }
    tokio::time::sleep(Duration::from_millis(hold_time)).await;
    // *guard += 1; need to drop before await
}

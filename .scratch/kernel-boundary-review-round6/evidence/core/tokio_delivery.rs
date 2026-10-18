// INPUT: production sessions and current-thread Tokio runtime
// OUTPUT: bounded evidence that one awaiting listener blocks another session driver
// POS: isolated round6 review evidence
use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_kernel_abi::{ConversationLog, SessionEvent, SessionEventListener};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
struct Count(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl SessionEventListener for Count {
 async fn on_event(&self,_:&SessionEvent) { self.0.fetch_add(1, Ordering::SeqCst); }
}
struct Park(Arc<tokio::sync::Notify>);
#[async_trait::async_trait]
impl SessionEventListener for Park {
 async fn on_event(&self,_:&SessionEvent) { self.0.notify_one(); std::future::pending::<()>().await; }
}
fn main() {
 let(tx,rx)=std::sync::mpsc::channel();
 std::thread::spawn(move|| {
  let rt=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
  rt.block_on(async {
   let a=ListenableInMemorySession::new();
   let b=ListenableInMemorySession::new();
   let count=Arc::new(AtomicUsize::new(0));
   b.subscribe(Arc::new(Count(count.clone()))).await;
   let entered=Arc::new(tokio::sync::Notify::new());
   a.subscribe(Arc::new(Park(entered.clone()))).await;
   a.append(SessionEvent::new_runtime("a")).await.unwrap();
   entered.notified().await;
   b.append(SessionEvent::new_runtime("b")).await.unwrap();
   tokio::time::sleep(std::time::Duration::from_millis(30)).await;
   tx.send(count.load(Ordering::SeqCst)).unwrap();
  });
 });
 let count=rx.recv_timeout(std::time::Duration::from_secs(2)).expect("runtime should make progress");
 println!("B deliveries while unrelated A listener awaits: {count}");
 assert_eq!(count,1,"unrelated B driver must deliver while A listener awaits");
}

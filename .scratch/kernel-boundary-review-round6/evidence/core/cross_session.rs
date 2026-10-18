// INPUT: production session backend and futures executor
// OUTPUT: cross-session inline delivery regression proof
// POS: isolated round6 review evidence
use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_kernel_abi::{ConversationLog, SessionEvent, SessionEventListener};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
struct Count(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl SessionEventListener for Count {
 async fn on_event(&self,_:&SessionEvent) { self.0.fetch_add(1, Ordering::SeqCst); }
}
struct Forward(Arc<ListenableInMemorySession>);
#[async_trait::async_trait]
impl SessionEventListener for Forward {
 async fn on_event(&self,_:&SessionEvent) { self.0.append(SessionEvent::new_runtime("forwarded")).await.unwrap(); }
}
fn main() { futures_executor::block_on(async {
 let a=ListenableInMemorySession::new();
 let b=Arc::new(ListenableInMemorySession::new());
 let count=Arc::new(AtomicUsize::new(0));
 b.subscribe(Arc::new(Count(count.clone()))).await;
 a.subscribe(Arc::new(Forward(b.clone()))).await;
 a.append(SessionEvent::new_runtime("outer")).await.unwrap();
 println!("B listener deliveries after successful A->B append: {}",count.load(Ordering::SeqCst));
 assert_eq!(count.load(Ordering::SeqCst),1,"cross-session append must deliver without requiring an unrelated future append");
}); }

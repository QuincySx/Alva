// INPUT: workspace session contracts and local async executor
// OUTPUT: bounded assertion of broadcast lifecycle contract
// POS: independent R25 review regression probe
use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_kernel_abi::{ConversationLog, SessionEvent, SessionEventListener};
use std::sync::{Arc,Weak};
struct Listener(Weak<ListenableInMemorySession>);
#[async_trait::async_trait]
impl SessionEventListener for Listener {
 async fn on_event(&self,event:&SessionEvent) {
  if event.event_type=="outer" {
   self.0.upgrade().unwrap().append(SessionEvent::new_runtime("inner")).await.unwrap();
  }
 }
}
fn main() {
 let(tx,rx)=std::sync::mpsc::channel();
 std::thread::spawn(move|| futures_executor::block_on(async {
  let session=Arc::new(ListenableInMemorySession::new());
  session.subscribe(Arc::new(Listener(Arc::downgrade(&session)))).await;
  session.append(SessionEvent::new_runtime("outer")).await.unwrap();
  tx.send(()).unwrap();
 }));
 assert!(rx.recv_timeout(std::time::Duration::from_secs(1)).is_ok(),"non-Tokio reentrant append deadlocked");
}

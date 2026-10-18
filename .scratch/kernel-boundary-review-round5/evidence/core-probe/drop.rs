// INPUT: workspace session contracts and local async executor
// OUTPUT: bounded assertion of broadcast lifecycle contract
// POS: independent R25 review regression probe
use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_kernel_abi::{ConversationLog, SessionEvent, SessionStore};
use futures_util::StreamExt;
fn main() {
 let rt=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
 rt.block_on(async {
  let session=ListenableInMemorySession::new();
  let mut stream=session.subscribe_events(0).await;
  session.append(SessionEvent::new_runtime("one")).await.unwrap();
  stream.next().await.unwrap();
  for _ in 0..10 { tokio::task::yield_now().await; }
  println!("before drop: {} tasks",rt.metrics().num_alive_tasks());
  session.close().await.unwrap(); drop(stream); drop(session);
  for _ in 0..100 { tokio::task::yield_now().await; }
  println!("after close/drop: {} tasks",rt.metrics().num_alive_tasks());
  assert_eq!(rt.metrics().num_alive_tasks(),0,"broadcast driver should terminate after session disposal");
 });
}

// INPUT: Hub control plane and in-memory gated fake EngineRuntime
// OUTPUT: deterministic obsolete cancel response vs fresh run claim race
// POS: isolated round6 review evidence
use alva_engine_runtime::*;
use alva_hub_core::*;
use futures_util::{stream, StreamExt};
use std::{pin::Pin,sync::{Arc,atomic::{AtomicUsize,Ordering}}};
struct Engine { turn:AtomicUsize, complete:Arc<tokio::sync::Notify>, cancel_enter:tokio::sync::Notify,cancel_release:tokio::sync::Notify }
#[async_trait::async_trait]
impl EngineRuntime for Engine {
 fn execute(&self,_:RuntimeRequest)->Result<Pin<Box<dyn futures_util::Stream<Item=RuntimeEvent>+Send>>,RuntimeError>{
  let n=self.turn.fetch_add(1,Ordering::SeqCst);
  let id=format!("turn-{n}");
  let started=stream::iter(vec![RuntimeEvent::SessionStarted{session_id:id.clone(),model:None,tools:vec![]}]);
  if n==0 {let gate=self.complete.clone(); Ok(Box::pin(started.chain(stream::once(async move {gate.notified().await; RuntimeEvent::Completed{session_id:id,result:None,usage:None}}))))}
  else {Ok(Box::pin(started.chain(stream::pending())))}
 }
 async fn cancel(&self,_:&str)->Result<(),RuntimeError>{self.cancel_enter.notify_one();self.cancel_release.notified().await;Err(RuntimeError::Unsupported("cancel".into()))}
 async fn respond_permission(&self,_:&str,_:&str,_:PermissionDecision)->Result<(),RuntimeError>{Ok(())}
 fn capabilities(&self)->RuntimeCapabilities{RuntimeCapabilities{streaming:true,tool_control:false,permission_callback:false,resume:true,cancel:true}}
}
fn main(){tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
 let hub=Arc::new(HubRuntime::new());
 let engine=Arc::new(Engine{turn:AtomicUsize::new(0),complete:Arc::new(tokio::sync::Notify::new()),cancel_enter:tokio::sync::Notify::new(),cancel_release:tokio::sync::Notify::new()});
 hub.register_engine("probe",engine.clone()).unwrap();
 let mut first=hub.execute(HubRequest::new("probe",RuntimeRequest::new("one"))).unwrap();
 let pid=first.next().await.unwrap().product_session_id;
 let cancel={let h=hub.clone();let p=pid.clone();tokio::spawn(async move{h.cancel(&p).await})};
 engine.cancel_enter.notified().await;
 engine.complete.notify_one();
 assert!(matches!(first.next().await.unwrap().event,RuntimeEvent::Completed{..}));
 let mut second=hub.execute(HubRequest::new("probe",RuntimeRequest::new("two")).resume(pid.clone())).unwrap();
 second.next().await.unwrap();
 assert_eq!(hub.session(&pid).unwrap().status,HubSessionStatus::Running);
 engine.cancel_release.notify_one(); cancel.await.unwrap().unwrap();
 let status=hub.session(&pid).unwrap().status;
 println!("fresh turn status after obsolete cancel returns Unsupported: {status:?}; live={}",hub.has_live_run(&pid));
 assert_eq!(status,HubSessionStatus::Running,"old cancel must not abandon the new generation");
});}

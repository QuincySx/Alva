// INPUT: HubRuntime and a synchronous execute gate in an in-memory engine
// OUTPUT: abandoned starting generation must not overwrite a newer RunHandle
// POS: isolated round6 review evidence
use alva_engine_runtime::*;
use alva_hub_core::*;
use futures_util::{stream,StreamExt};
use std::{pin::Pin,sync::{Arc,Mutex,Condvar,atomic::{AtomicUsize,Ordering}}};
struct Engine {turn:AtomicUsize, entered:tokio::sync::Notify,gate:(Mutex<bool>,Condvar)}
#[async_trait::async_trait]
impl EngineRuntime for Engine {
 fn execute(&self,_:RuntimeRequest)->Result<Pin<Box<dyn futures_util::Stream<Item=RuntimeEvent>+Send>>,RuntimeError>{
  if self.turn.fetch_add(1,Ordering::SeqCst)==0 {
   self.entered.notify_one();let mut released=self.gate.0.lock().unwrap();while !*released {released=self.gate.1.wait(released).unwrap();}
   Ok(Box::pin(stream::pending()))
  }else{Ok(Box::pin(stream::iter(vec![RuntimeEvent::SessionStarted{session_id:"new-native".into(),model:None,tools:vec![]}]).chain(stream::pending())))}
 }
 async fn cancel(&self,_:&str)->Result<(),RuntimeError>{Ok(())}
 async fn respond_permission(&self,_:&str,_:&str,_:PermissionDecision)->Result<(),RuntimeError>{Ok(())}
 fn capabilities(&self)->RuntimeCapabilities{RuntimeCapabilities{streaming:true,tool_control:false,permission_callback:false,resume:true,cancel:true}}
}
fn main(){let rt=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();rt.block_on(async{
 let hub=Arc::new(HubRuntime::new());
 let e=Arc::new(Engine{turn:AtomicUsize::new(0),entered:tokio::sync::Notify::new(),gate:(Mutex::new(false),Condvar::new())});hub.register_engine("probe",e.clone()).unwrap();
 let (tx,rx)=std::sync::mpsc::channel();
 {let h=hub.clone();let runtime=tokio::runtime::Handle::current(); std::thread::spawn(move||{let _enter=runtime.enter();tx.send(h.execute(HubRequest::new("probe",RuntimeRequest::new("old"))).unwrap()).unwrap();});}
 e.entered.notified().await;
 let pid=hub.sessions()[0].product_session_id.clone();
 hub.abandon(&pid).unwrap();
 let mut second=hub.execute(HubRequest::new("probe",RuntimeRequest::new("new")).resume(pid.clone())).unwrap();second.next().await.unwrap();
 assert!(hub.has_live_run(&pid));
 *e.gate.0.lock().unwrap()=true;e.gate.1.notify_one();
 let _old=rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
 tokio::time::sleep(std::time::Duration::from_millis(30)).await;
 println!("new generation after abandoned execute finally registers: status={:?}, live={}",hub.session(&pid).unwrap().status,hub.has_live_run(&pid));
 assert!(hub.has_live_run(&pid),"late abandoned generation must not erase new run control plane");
});}

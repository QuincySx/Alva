// INPUT: Real Hub run seam and verbatim current HTTP timeline projector body
// OUTPUT: Deterministic evidence that Failed is not engine stream termination
// POS: Temporary review probe, archived outside production tests after execution
use std::{collections::HashSet,pin::Pin,sync::{Arc,Mutex}};
use async_trait::async_trait;
use futures_core::Stream;
use alva_engine_runtime::{EngineRuntime,RuntimeRequest,RuntimeEvent,RuntimeError,RuntimeCapabilities,PermissionDecision};
use alva_hub_desktop::{HubStore,TimelineEvent,TimelineStatus,RunRequest,run_persisted_with_target,PermissionRouter};
#[derive(Clone)]
struct WireTimelineEvent { event:TimelineEvent, run_token:Option<String> }
fn timeline_now_ms()->i64 {0}
// Body below is copied verbatim by the review generator from server/main.rs.
fn project(event:TimelineEvent, stopping:&Mutex<HashSet<String>>,stop_store:&HubStore,tx:&tokio::sync::broadcast::Sender<WireTimelineEvent>,permission_router:&PermissionRouter){
 let run_token:Option<String>=None;
                if matches!(
                    event.status,
                    alva_hub_desktop::TimelineStatus::Completed
                        | alva_hub_desktop::TimelineStatus::Failed
                        | alva_hub_desktop::TimelineStatus::Interrupted
                        | alva_hub_desktop::TimelineStatus::Fallback
                        | alva_hub_desktop::TimelineStatus::Exhausted
                ) {
                    permission_router.clear_session(&event.product_session_id);
                    // R51: a Stop that was acked records its cancelled
                    // terminal here — when the engine stream really ended —
                    // never on the cancel ack itself.
                    if stopping
                        .lock()
                        .unwrap()
                        .remove(&event.product_session_id)
                    {
                        let _ = stop_store.record_attempt_cancelled(
                            &event.product_session_id,
                            "User stopped",
                        );
                        let _ = tx.send(WireTimelineEvent {
                            event: TimelineEvent {
                                created_at: timeline_now_ms(),
                                status: alva_hub_desktop::TimelineStatus::Interrupted,
                                engine_id: event.engine_id.clone(),
                                product_session_id: event.product_session_id.clone(),
                                message: Some("User stopped".into()),
                                item_id: None,
                                activity: None,
                                permission_request: None,
                            },
                            run_token: run_token.clone(),
                        });
                    }
                }
                let wire = WireTimelineEvent {
                    event: event.clone(),
                    run_token: run_token.clone(),
                };
                let _ = tx.send(wire);
}
struct Fixture { fail:Arc<tokio::sync::Notify>, finish:Arc<tokio::sync::Notify> }
#[async_trait]
impl EngineRuntime for Fixture {
 fn execute(&self,_:RuntimeRequest)->Result<Pin<Box<dyn Stream<Item=RuntimeEvent>+Send>>,RuntimeError>{
  let fail=self.fail.clone(); let finish=self.finish.clone(); let (tx,rx)=tokio::sync::mpsc::channel(8);
  tokio::spawn(async move {
   tx.send(RuntimeEvent::SessionStarted{session_id:"fixture-native".into(),model:None,tools:vec![]}).await.unwrap();
   fail.notified().await;
   tx.send(RuntimeEvent::Error{message:"Cancelled by user".into(),recoverable:false}).await.unwrap();
   finish.notified().await;
   let _=tx.send(RuntimeEvent::Completed{session_id:"fixture-native".into(),result:None,usage:None}).await;
  });
  Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
 }
 async fn cancel(&self,_:&str)->Result<(),RuntimeError>{Ok(())}
 async fn respond_permission(&self,_:&str,_:&str,_:PermissionDecision)->Result<(),RuntimeError>{Ok(())}
 fn capabilities(&self)->RuntimeCapabilities {RuntimeCapabilities{streaming:true,tool_control:false,permission_callback:false,resume:true,cancel:true}}
}
#[tokio::test]
async fn failed_projection_must_not_report_interrupted_before_completed(){
 let workspace=tempfile::tempdir().unwrap();let data=tempfile::tempdir().unwrap();let store=HubStore::open(data.path().join("test.db")).unwrap();
 let fail=Arc::new(tokio::sync::Notify::new());let finish=Arc::new(tokio::sync::Notify::new());
 let engine=Arc::new(Fixture{fail:fail.clone(),finish:finish.clone()});
 let stopping=Arc::new(Mutex::new(HashSet::new()));let (tx,mut rx)=tokio::sync::broadcast::channel(64);
 let stopping_run=stopping.clone();let run_store=store.clone();let workspace_path=workspace.path().to_path_buf();
 let run=tokio::spawn(async move {
  let router=PermissionRouter::new();let project_store=run_store.clone();
  run_persisted_with_target(&run_store,move |_,_,_| Ok(engine.clone() as Arc<dyn EngineRuntime>), |_|None,
   RunRequest{prompt:"fixture".into(),workspace:workspace_path,engine_id:"alva".into(),product_session_id:None,permission_profile:None},None,None,
   move |event|project(event,&stopping_run,&project_store,&tx,&router)).await
 });
 let id=loop {let wire=rx.recv().await.unwrap();if matches!(wire.event.status,TimelineStatus::Running){break wire.event.product_session_id;}};
 stopping.lock().unwrap().insert(id);
 fail.notify_one();let mut premature=false;
 loop {let wire=rx.recv().await.unwrap();if matches!(wire.event.status,TimelineStatus::Interrupted){premature=true;}if matches!(wire.event.status,TimelineStatus::Failed){break;}}
 let run_still_open=!run.is_finished();
 finish.notify_one();run.await.unwrap().unwrap();
 eprintln!("Interrupted before Completed = {premature}; real run still open = {run_still_open}");
 assert!(!premature,"HTTP projector must not interpret Failed as actual stream termination");
}

struct StartFailure;
#[async_trait]
impl EngineRuntime for StartFailure {
 fn execute(&self,_:RuntimeRequest)->Result<Pin<Box<dyn Stream<Item=RuntimeEvent>+Send>>,RuntimeError>{Err(RuntimeError::NotReady("fixture startup rejection".into()))}
 async fn cancel(&self,_:&str)->Result<(),RuntimeError>{Ok(())}
 async fn respond_permission(&self,_:&str,_:&str,_:PermissionDecision)->Result<(),RuntimeError>{Ok(())}
 fn capabilities(&self)->RuntimeCapabilities{RuntimeCapabilities{streaming:true,tool_control:false,permission_callback:false,resume:true,cancel:true}}
}
#[tokio::test]
async fn synchronous_start_failure_releases_published_hub(){
 let workspace=tempfile::tempdir().unwrap();let data=tempfile::tempdir().unwrap();let store=HubStore::open(data.path().join("test.db")).unwrap();
 let hubs:alva_hub_desktop::ActiveHubs=Mutex::new(std::collections::HashMap::new());
 let outcome=run_persisted_with_target(&store,|_,_,_|Ok(Arc::new(StartFailure) as Arc<dyn EngineRuntime>),|_|None,
  RunRequest{prompt:"fixture".into(),workspace:workspace.path().to_path_buf(),engine_id:"alva".into(),product_session_id:None,permission_profile:None},None,Some(&hubs),|_|{}).await.unwrap();
 assert!(outcome.failed);
 let leaked=hubs.lock().unwrap().len();eprintln!("published live hubs after synchronous start failure: {leaked}");
 assert_eq!(leaked,0,"failed execute must release the published control plane");
}

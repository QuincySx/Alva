// INPUT: HubRuntime public API and deterministic in-process engine fixtures
// OUTPUT: Round-5 lifecycle regression evidence
// POS: Temporary review integration probes, archived under scratch after use
use std::{pin::Pin,sync::Arc};
use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use alva_engine_runtime::{EngineRuntime,RuntimeRequest,RuntimeEvent,RuntimeError,RuntimeCapabilities,PermissionDecision};
use alva_hub_core::{HubRuntime,HubRequest,HubSessionStatus};
struct Fixture { pending: bool }
#[async_trait]
impl EngineRuntime for Fixture {
 fn execute(&self,_:RuntimeRequest)->Result<Pin<Box<dyn Stream<Item=RuntimeEvent>+Send>>,RuntimeError>{
   let start=RuntimeEvent::SessionStarted{session_id:"native".into(),model:None,tools:vec![]};
   if self.pending { return Ok(Box::pin(stream::iter(vec![start]).chain(stream::pending()))); }
   Ok(Box::pin(stream::iter(vec![start,
     RuntimeEvent::ToolStart{id:"tool".into(),name:"read".into(),input:serde_json::json!({})},
     RuntimeEvent::Error{message:"fixture failure".into(),recoverable:false},
     RuntimeEvent::Completed{session_id:"native".into(),result:None,usage:None}])))
 }
 async fn cancel(&self,_:&str)->Result<(),RuntimeError>{Err(RuntimeError::Unsupported("cancel".into()))}
 async fn respond_permission(&self,_:&str,_:&str,_:PermissionDecision)->Result<(),RuntimeError>{Ok(())}
 fn capabilities(&self)->RuntimeCapabilities{RuntimeCapabilities{streaming:true,tool_control:false,permission_callback:false,resume:true,cancel:false}}
}
#[tokio::test]
async fn closes_unfinished_tools_before_forwarding_completed(){
 let hub=HubRuntime::new();hub.register_engine("fixture",Arc::new(Fixture{pending:false})).unwrap();
 let events:Vec<_>=hub.execute(HubRequest::new("fixture",RuntimeRequest::new("probe"))).unwrap().collect().await;
 let completed=events.iter().position(|e|matches!(e.event,RuntimeEvent::Completed{..})).unwrap();
 let tool_end=events.iter().position(|e|matches!(e.event,RuntimeEvent::ToolEnd{..})).unwrap();
 eprintln!("completed index={completed}, tool_end index={tool_end}");
 assert!(tool_end<completed,"unfinished tool must close before Completed");
}
#[tokio::test]
async fn unsupported_cancel_still_releases_the_hub_claim(){
 let hub=HubRuntime::new();hub.register_engine("fixture",Arc::new(Fixture{pending:true})).unwrap();
 let mut events=hub.execute(HubRequest::new("fixture",RuntimeRequest::new("probe"))).unwrap();
 let first=events.next().await.unwrap();let id=first.product_session_id;
 let result=hub.cancel(&id).await;
 let status=hub.session(&id).unwrap().status;
 hub.abandon(&id).unwrap();
 eprintln!("cancel={result:?}; status={status:?}");
 assert!(result.is_ok(),"unsupported engine cancel should use Hub abandon fallback");
 assert_ne!(status,HubSessionStatus::Running);
}

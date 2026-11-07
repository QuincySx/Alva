// INPUT: Final SmallPen Product package and installed macOS SmallPen App
// OUTPUT: App readiness, canvas observation and unchanged-revision evidence
// POS: Disposable-profile, read-only App inspection; records the observed tool versions.
import {spawn,execFileSync} from 'node:child_process';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
const root=process.cwd()+'/docs/design/alva';
const dir='/tmp/alva-design-authoring/app-opening-'+Date.now();await mkdir(dir,{recursive:true});
const profile=dir+'/profile-'+Date.now(),report=dir+'/smoke.json';await mkdir(profile,{recursive:true});
const revision=()=>JSON.parse(execFileSync('smallpen',['project','show',root+'/alva.smallpen','--json'],{encoding:'utf8'})).revision;
const before=revision(),env={...process.env,SMALLPEN_SMOKE_PROFILE:profile,SMALLPEN_SMOKE_REPORT:report,SMALLPEN_APPLICATION_STATE_PATH:profile+'/state.json'};delete env.ELECTRON_RUN_AS_NODE;
const child=spawn('/Applications/SmallPen.app/Contents/MacOS/SmallPen',['--smoke','--remote-debugging-port=0',root+'/alva.smallpen'],{env,stdio:['ignore','pipe','pipe']});
let logs='';for(const stream of [child.stdout,child.stderr])stream.on('data',b=>{if(logs.length<1_000_000)logs+=b});
const delay=ms=>new Promise(r=>setTimeout(r,ms));
const exited=new Promise(r=>child.once('exit',(code,signal)=>r({code,signal})));
let socket;
async function observe(){
 const deadline=Date.now()+100000;let port,target;
 while(Date.now()<deadline){
  try{port=(await readFile(profile+'/DevToolsActivePort','utf8')).split('\n')[0];const list=await(await fetch('http://127.0.0.1:'+port+'/json/list')).json();target=list.find(x=>x.type==='page'&&x.url.startsWith('http://127.0.0.1:'));if(target)break}catch{}
  await delay(100);
 }
 if(!target)throw Error('App debug target unavailable');
 socket=new WebSocket(target.webSocketDebuggerUrl);await new Promise((r,j)=>{socket.addEventListener('open',r,{once:true});socket.addEventListener('error',j,{once:true})});
 let next=0;const pending=new Map();socket.addEventListener('message',e=>{const m=JSON.parse(e.data);if(m.id&&pending.has(m.id)){const {resolve,reject}=pending.get(m.id);pending.delete(m.id);m.error?reject(Error(m.error.message)):resolve(m.result)}});
 const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++next;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));setTimeout(()=>{if(pending.has(id)){pending.delete(id);reject(Error('Timed out: '+method))}},5000)});
 let state;
 while(Date.now()<deadline){
  state=(await send('Runtime.evaluate',{expression:'({viewportReady:Boolean(document.querySelector("[data-testid=viewport]")),canvasCount:document.querySelectorAll("canvas").length,text:document.body.innerText.slice(0,6500),title:document.title})',returnByValue:true})).result.value;
  if(state.viewportReady)break;await delay(100);
 }
 const selection = await send('Runtime.evaluate',{expression:'(()=>{const e=[...document.querySelectorAll("span")].find(x=>x.textContent==="Conversation · desktop");if(!e)return {found:false};const html=e.outerHTML;e.scrollIntoView({block:"center"});e.dispatchEvent(new MouseEvent("click",{bubbles:true}));return {found:true,html}})()',returnByValue:true});state.selection=selection.result.value;
 await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Shift',code:'ShiftLeft',modifiers:8,windowsVirtualKeyCode:16});
 await send('Input.dispatchKeyEvent',{type:'keyDown',key:'@',code:'Digit2',modifiers:8,windowsVirtualKeyCode:50});
 await send('Input.dispatchKeyEvent',{type:'keyUp',key:'@',code:'Digit2',modifiers:8,windowsVirtualKeyCode:50});
 await send('Input.dispatchKeyEvent',{type:'keyUp',key:'Shift',code:'ShiftLeft',modifiers:0,windowsVirtualKeyCode:16});
 await delay(1600);
 const screenshot=await send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});
 await writeFile(root+'/exports/archive/app-opening.png',Buffer.from(screenshot.data,'base64'));
 await writeFile(dir+'/observation.json',JSON.stringify(state,null,2)+'\n');socket.close();
 return state;
}
const observed=observe().then(value=>({status:'fulfilled',value}),error=>({status:'rejected',error:String(error)}));
const exit=await exited;const observation=await observed;await writeFile(dir+'/stdout.txt',logs);
let smoke;try{smoke=JSON.parse(await readFile(report,'utf8'))}catch(error){smoke={status:'unavailable',error:String(error)}}
const after=revision();const result={version:execFileSync('plutil',['-extract','CFBundleShortVersionString','raw','/Applications/SmallPen.app/Contents/Info.plist'],{encoding:'utf8'}).trim(),cliVersion:JSON.parse(execFileSync('smallpen',['version','--json'],{encoding:'utf8'})).version,beforeRevision:before,afterRevision:after,unchanged:before===after,exit,smoke:{status:smoke.status,ui:smoke.ui,startupMs:smoke.startupMs,message:smoke.message},observation,screenshot:observation.status==='fulfilled'?'exports/archive/app-opening.png':null,scope:'Final installed App opening and current viewport observation only; no all-page pixel parity claim.'};
await writeFile(root+'/exports/archive/app-opening.json',JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result));

// INPUT: Saved offline prototype and a disposable local Chrome profile
// OUTPUT: Browser hotspot/focus/flow/local-asset checks and a screenshot
// POS: macOS design-viewer QA; does not test production services or call models.
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const root=process.cwd()+'/docs/design/alva',profile='/tmp/alva-design-authoring/chrome-'+Date.now();await mkdir(profile,{recursive:true});
const child=spawn('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',['--headless=new','--no-first-run','--no-default-browser-check','--disable-background-networking','--disable-extensions','--window-size=1800,1100','--force-device-scale-factor=1','--remote-debugging-port=0','--user-data-dir='+profile,'about:blank'],{stdio:'ignore'});
const exited=new Promise(r=>child.on('exit',r)),delay=ms=>new Promise(r=>setTimeout(r,ms));let ws,next=0;const pending=new Map(),errors=[],requests=[],checks=[];
const assert=(condition,name)=>{checks.push({name,status:condition?'pass':'fail'});if(!condition)throw Error(name)};
try{
 let port,target;for(let i=0;i<200;i++){try{port=(await readFile(profile+'/DevToolsActivePort','utf8')).split('\n')[0];target=(await(await fetch('http://127.0.0.1:'+port+'/json/list')).json()).find(x=>x.type==='page');if(target)break}catch{}await delay(100)}
 if(!target)throw Error('Headless browser unavailable');ws=new WebSocket(target.webSocketDebuggerUrl);await new Promise((r,j)=>{ws.addEventListener('open',r,{once:true});ws.addEventListener('error',j,{once:true})});
 ws.addEventListener('message',e=>{const m=JSON.parse(e.data);if(m.method==='Runtime.exceptionThrown')errors.push(m.params.exceptionDetails.text);if(m.method==='Network.requestWillBeSent')requests.push(m.params.request.url);if(pending.has(m.id)){const [r,j,t]=pending.get(m.id);clearTimeout(t);pending.delete(m.id);m.error?j(Error(m.error.message)):r(m.result)}});
 const send=(method,params={})=>new Promise((r,j)=>{const id=++next,t=setTimeout(()=>{pending.delete(id);j(Error('Timeout: '+method))},10000);pending.set(id,[r,j,t]);ws.send(JSON.stringify({id,method,params}))});
 const evaluate=async expression=>{const r=await send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value};
 await send('Runtime.enable');await send('Network.enable');await send('Page.enable');await send('Page.navigate',{url:pathToFileURL(root+'/preview.html').href});
 for(let i=0;i<100;i++){try{if(await evaluate('typeof DATA!=="undefined"&&document.readyState==="complete"'))break}catch{}await delay(100)}
 const settled=()=>evaluate('(async()=>{await Promise.all([...document.images].map(i=>i.decode()));await new Promise(requestAnimationFrame);return {page,overlay,system,focus:document.activeElement.dataset.element||document.activeElement.id,combo:combo()}})()');
 const select=async target=>{await evaluate('(()=>{const e=document.getElementById("page");e.value='+JSON.stringify(target)+';e.dispatchEvent(new Event("change",{bubbles:true}))})()');return settled()};
 const click=async element=>{const ok=await evaluate('(()=>{const e=document.querySelector(\'.hotspot[data-element="'+element+'"]\');if(!e)return false;e.click();return true})()');assert(ok,'hotspot exists: '+element);return settled()};
 let s=await settled();assert(s.page==='Workspace / Overview','initial overview and image loaded');
 await select('Chat / WaitingQuestion');s=await click('AnswerQuestion');assert(s.overlay==='Overlay / Question','question opens as overlay');
 await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});s=await settled();assert(s.overlay===null&&s.focus==='AnswerQuestion','Escape closes and restores question trigger focus');
 await click('AnswerQuestion');s=await click('OptionOther');assert(s.overlay==='Overlay / QuestionOther','Other opens custom-answer state');
 s=await click('QuestionSubmit');assert(s.page==='Operations / AnsweredWaits'&&s.overlay===null,'save answer keeps task paused');
 s=await click('ResumeOriginal');assert(s.page==='Chat / Running','explicit continue displays running');s=await click('StopTask');assert(s.page==='Chat / Stopped','explicit stop displays stopped');
 await select('Chat / ApprovalPending');await click('ReviewApproval');s=await click('ApprovalDeny');assert(s.page==='Chat / Stopped','permission denial separate from answer submission');
 await select('Chat / Conversation');await click('OpenInspector');const drawer=await evaluate('(()=>{const c=document.getElementById("overlay-card"),r=c.getBoundingClientRect(),a=document.getElementById("screen").getBoundingClientRect();return {role:c.getAttribute("role"),modal:c.getAttribute("aria-modal"),right:Math.abs(r.right-a.right)}})()');assert(drawer.role==='complementary'&&drawer.modal==='false'&&drawer.right<2,'Inspector is a nonmodal drawer anchored to app frame');
 await click('CloseOverlay');await select('Settings / General');await evaluate('(()=>{const e=document.getElementById("theme");e.value="dark";e.dispatchEvent(new Event("change"));const w=document.getElementById("width");w.value="narrow";w.dispatchEvent(new Event("change"))})()');s=await settled();assert(s.combo==='dark-narrow','theme/viewport select native exported combination');
 await select('Developer / IntegrationTests');s=await click('EvaluationTab');assert(s.page==='Developer / Evaluation','tests and evaluation remain separate');
 await select('Chat / Conversation');await evaluate('(()=>{const e=document.getElementById("width");e.value="desktop";e.dispatchEvent(new Event("change"))})()');await settled();
 const png=await send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(root+'/exports/archive/preview-browser.png',Buffer.from(png.data,'base64'));
 assert(errors.length===0,'no browser JavaScript exceptions');assert(requests.every(x=>x.startsWith('file:')||x.startsWith('data:')),'preview only loads local assets; no backend/model requests');
 const result={status:'pass',sourceRevision:await evaluate('DATA.revision'),previewSHA256:createHash('sha256').update(await readFile(root+'/preview.html')).digest('hex'),scope:'Offline design viewer only; not production UI or backend validation',checks,requests:requests.length,externalRequests:requests.filter(x=>!x.startsWith('file:')&&!x.startsWith('data:')),errors,screenshot:'exports/archive/preview-browser.png'};await writeFile(root+'/exports/archive/preview-browser.json',JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result));
 await send('Browser.close');ws.close();await exited;
}catch(error){await writeFile(root+'/exports/archive/preview-browser.json',JSON.stringify({status:'fail',error:String(error),checks,errors},null,2)+'\n');console.error(error);ws?.close();child.kill('SIGTERM');process.exitCode=1}

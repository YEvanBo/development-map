import {test} from 'node:test';
import assert from 'node:assert/strict';
import {MapNavigation} from './navigation.mjs';
const target = node_ref => ({project_id:'p',revision:'v1',node_ref});
const load = async v => ({...v,node:{node_ref:v.node_ref,node_version:1},parent_ref:'arch:parent',core_architecture_ref:'architecture:p:v1'});
test('back/forward differ from parent/core; view state survives branch changes', async () => {
 const n=new MapNavigation(load); await n.go(target('dev:a'));
 n.remember({expanded:['dev:x'],pages:{a:'cursor'},camera:{x:10,zoom:2},nodeVersion:1});
 await n.go(target('dev:b')); const back=await n.back();
 assert.equal(back.current.node_ref,'dev:a'); assert.equal(back.view.camera.zoom,2); assert.equal(back.view.pages.a,'cursor');
 await n.forward(); assert.equal(n.current.node_ref,'dev:b');
 await n.up(); assert.equal(n.current.node_ref,'arch:parent');
 await n.core(); assert.equal(n.current.node_ref,'architecture:p:v1');
 const restored=new MapNavigation(load); await restored.openUrl(n.url()); assert.deepEqual(restored.current,n.current);
});
test('late responses cannot overwrite newest navigation',async()=>{
 let resolve; const n=new MapNavigation(v=>v.node_ref==='slow'?new Promise(r=>resolve=()=>r(load(v))):load(v));
 const slow=n.go(target('slow')); await n.go(target('fast')); resolve(); await slow;
 assert.equal(n.current.node_ref,'fast'); assert.equal(n.entries.length,1);
});
test('changed branch discards stale pagination but retains camera and expansion',async()=>{
 const n=new MapNavigation(load);await n.go(target('dev:a'));n.remember({pages:{a:1},expanded:['x'],camera:{x:8},nodeVersion:0});
 const r=await n.go(target('dev:a'));assert.deepEqual(r.view.pages,{});assert.equal(r.view.camera.x,8);
});
test('loader assembles paged breadcrumbs in root-to-focus order',async()=>{
 const {createHttpLoader}=await import('./navigation.mjs');let calls=0;
 const loader=createHttpLoader(async()=>++calls===1?{project_id:'p',revision:'v1',node:{node_ref:'dev:m'},breadcrumbs:[{node_ref:'dev:m'}],ancestors_next:'cursor'}:{items:[{node_ref:'arch:a'},{node_ref:'project:p'}],next:null});
 const focus=await loader(target('dev:m'),new AbortController().signal);
 assert.deepEqual(focus.breadcrumbs.map(v=>v.node_ref),['project:p','arch:a','dev:m']);assert.equal(calls,2);
});

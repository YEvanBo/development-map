"""Actual frozen-compatible Core MCP Session + temp DB, not native Agent E2E."""
import copy
import json
from pathlib import Path
import subprocess
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from workflow import run

binary=Path(sys.argv[1]).resolve()
process=subprocess.Popen([str(binary)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
log=[]
calls=0

def exchange(q):
    process.stdin.write(json.dumps(q)+'\n');process.stdin.flush()
    line=process.stdout.readline()
    if not line:raise RuntimeError('fixture driver exited')
    return json.loads(line)
def core(name,args):
    global calls
    calls+=1
    return exchange({'name':name,'arguments':args})
def workflow(action,args):
    result=run({'action':action,'arguments':args},core)
    log.append({'action':action,'result':result})
    return result
def submit(base,version,key,generation=1):
    return {'submission':{'submission_key':key,'base_checkpoint_rowid':base,
        'checkpoint':{'node_id':'n','owner':'fixture','generation':generation,'version':version,
        'digest':'0123456789abcdef','summary':'JSON plus LF','source_ref':'fixture'},'artifacts':[]}}
try:
    missing=workflow('resume',{'project_id':'p','task_id':'unknown'})
    assert missing['reason']=='NO_BINDING'
    initial=workflow('resume',{'project_id':'p'})
    assert initial['status']=='CONFIRM_GOAL' and initial['capabilities']['submit'] is False
    assert workflow('submit',submit(0,1,'before-confirm'))['status']=='CORE_ERROR'
    confirmation=exchange({'fixture':'confirm','value':{'project_id':'p','node_id':'n','generation':1,'previous':0,'goal':'JSON plus LF','user_ref':'fixture-user-1'}})['event']
    before=calls;direct=core('map_resume',{'project_id':'p'});direct_calls=calls-before
    before=calls;active=workflow('resume',{'project_id':'p'});facade_calls=calls-before
    assert active['status']=='CONTINUE' and active['capabilities']['submit'] is True
    assert active['goal_revision']==confirmation
    direct_value=json.loads(direct['content'][0]['text'])
    assert active['context']==direct_value and active['capabilities']==direct_value['capabilities']
    original=submit(active['checkpoint'],active['version']+1,'first')
    saved=workflow('submit',original);assert saved['status']=='SUBMITTED'
    replay=workflow('submit',original);assert replay['status']=='REPLAYED'
    assert replay['receipt']['event_id']==saved['receipt']['event_id']
    wrong=copy.deepcopy(original);wrong['submission']['checkpoint']['summary']='changed'
    assert workflow('submit',wrong)['reason']=='CONFLICT'
    stale=submit(active['checkpoint'],active['version']+2,'stale-session')
    assert workflow('submit',stale)['reason']=='CONFLICT'
    head=workflow('resume',{'project_id':'p'})
    assert workflow('submit',submit(head['checkpoint'],head['version']+1,'stale-generation',2))['status']=='CORE_ERROR'
    exchange({'fixture':'revoke','value':{'project_id':'p','node_id':'n','previous':confirmation,'user_ref':'fixture-revocation'}})
    revoked=workflow('resume',{'project_id':'p'})
    assert revoked['status']=='CONFIRM_GOAL' and not revoked['capabilities']['submit']
    assert workflow('submit',submit(head['checkpoint'],head['version']+1,'revoked-write'))['status']=='CORE_ERROR'
    exchange({'fixture':'confirm','value':{'project_id':'p','node_id':'n','generation':1,'previous':revoked['goal_revision'],'goal':'JSON plus LF','user_ref':'fixture-user-2'}})
    head=workflow('resume',{'project_id':'p'})
    final=submit(head['checkpoint'],head['version']+1,'final')
    final['submission']['resources']={'architecture_revision':'v1','hierarchy_exception':'','entries':[{'key':c,'category':c,'ownership':'primary','required':True,'not_applicable':True,'reason':'Logical state-machine fixture; no product artifact claim'} for c in ('directory','code','document','database','checkpoint')]}
    assert workflow('submit',final)['status']=='SUBMITTED'
    exchange({'fixture':'accept','version':final['submission']['checkpoint']['version'],'digest':'0123456789abcdef'})
    accepted=workflow('resume',{'project_id':'p'})
    assert accepted['status']=='COMPLETED' and not accepted['capabilities']['submit']
    assert workflow('submit',final)['status']=='REPLAYED'
    assert workflow('submit',submit(accepted['checkpoint'],accepted['version']+1,'after-accept'))['reason']=='COMPLETED'
    count=calls
    for action in ('intake','accept','revoke','reopen','verify','correction.propose','correction.apply','correction.history'):
        assert workflow(action,{})['status']=='BLOCKED'
    assert calls==count
    metrics={'direct_calls':direct_calls,'facade_calls':facade_calls,'calls_saved':direct_calls-facade_calls,
             'direct_json_bytes':len(json.dumps(direct_value).encode()),'facade_json_bytes':len(json.dumps(active).encode()),
             'tokens':'NOT_MEASURED','native_agent_sessions':'NOT_TESTED',
             'interpretation':'Core resume already aggregates state. No round-trip saving; facade adds envelope overhead but exposes stable selectors and deterministic prompts.'}
    print(json.dumps({'result':'PASS','core_tool_calls':calls,'metrics':metrics,'cases':log},indent=2))
finally:
    process.stdin.close();process.wait(timeout=10)
    if process.returncode:raise RuntimeError(f'fixture exit {process.returncode}')

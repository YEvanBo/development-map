#!/usr/bin/env python3
"""EvanBo / Apache-2.0. Operator-run, conservative discovery installer.
No database access, host execution, permission elevation or background service.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import shlex
import sys
import tempfile

VERSION = 3
START = '<!-- development-map:discovery:v1 -->'
END = '<!-- /development-map:discovery:v1 -->'


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(',', ':'))


def safe(path):
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts or any(ord(c) < 32 for c in str(path)):
        raise ValueError('absolute normalized path required')
    for p in [*reversed(path.parents), path]:
        if p.is_symlink():
            raise ValueError('symbolic link refused: ' + str(p))
        if p.exists() and p.is_file() and p.stat().st_nlink != 1:
            raise ValueError('hard link refused: ' + str(p))
    return path


def read(path):
    path = safe(path)
    if not path.exists():
        return None
    if not path.is_file() or path.stat().st_size > 1024 * 1024:
        raise ValueError('expected regular file <= 1 MiB: ' + str(path))
    return path.read_bytes().decode('utf-8')


def snapshot(path):
    content = read(path)
    return {'content': content, 'mode': stat.S_IMODE(path.stat().st_mode) if content is not None else None}


def managed(text):
    text = text or ''
    if text.count(START) != text.count(END) or text.count(START) > 1:
        raise ValueError('ambiguous managed markers')
    if START not in text:
        return None
    a, b = text.index(START), text.index(END) + len(END)
    if text.index(END) <= a:
        raise ValueError('invalid managed markers')
    return text[a:b]


def locations(root, host):
    if host not in ('codex', 'dsh'):
        raise ValueError('UNSUPPORTED host')
    root = safe(root)
    if os.name != 'posix':
        raise ValueError('UNSUPPORTED: permission and atomic replace adapter requires POSIX; Windows not verified')
    if not root.is_dir():
        raise ValueError('existing root directory required')
    # Explicit root is either the project or the actual selected host home.
    name = 'AGENTS.override.md' if host == 'codex' and (root / 'AGENTS.override.md').exists() else 'AGENTS.md'
    return root / name, root / ('.development-map-install-' + host + '.json')


def rule(mode, scope, root=None, host=None):
    policy = ('Do not automatically invoke Development Map for this scope.' if mode == 'disabled' else
              'Inherit the selected host global discovery policy; if absent, do not automatically invoke the map.' if mode == 'inherit' else
              'For engineering assignments only, discover the configured Development Map binding before editing; ordinary questions do not trigger discovery.')
    runtime = '.agents/skills/development-map/scripts/map-status.py'
    command = ('python3 ' + runtime + ' status --root "$(pwd -P)"' if scope == 'project' else
               'python3 ' + shlex.quote(str(Path(root) / runtime)) + ' status --root ' + shlex.quote(str(root)))
    command += ' --host ' + str(host) + ' --scope ' + scope
    return START + '\n' + policy + '\nBefore automatic map discovery for an engineering task, from the directory containing this instruction file run `' + command + '`. If effective_enabled is false or status fails, do not automatically call the map. Ordinary questions do not run this check.\n' + (
        'Project policy overrides global policy. Before each automatic invocation, re-read applicable managed policy; a disabled project stops discovery. '
        'Enabling is not authorization: never infer project/node identity from paths, create nodes, or bypass user goal confirmation. '
        'Use only already configured authorized MCP/CLI access; if unavailable report missing setup without searching for credentials. '
        'Saved gaps are not new work. Deliver and stop after current acceptance. '
        'MD is guidance, not an access-control boundary. Existing sessions may require restart.\n') + END


def discover(root, host, scope, local_only=False):
    entry, state = locations(root, host)
    record = json.loads(read(state) or '{}')
    if record and (record.get('scope') != scope or record.get('host') != host or record.get('entry') != str(entry)):
        raise ValueError('installation identity conflict')
    intact=bool(record) and managed(read(entry))==record.get('block')
    for rel,info in record.get('assets',{}).items():
        content=read(asset_path(Path(root),rel))
        intact=intact and content is not None and digest(content)==info['sha256']
    return {'host': host, 'scope': scope, 'root': str(root), 'entry': str(entry),
            'installed': intact, 'installation_record_present':bool(record), 'files_verified':intact, 'installer_version':record.get('installer_version'), 'skill_hashes':record.get('assets',{}), 'enabled': record.get('mode', 'inherit'),
            'effective_enabled': None if local_only else intact and effective(record, host, scope), 'install_mode':record.get('flavor','rules-only'),
            'authorized': 'NOT_CHECKED', 'files_present': intact,
            'policy_enforcement':'NOT_TESTED', 'host_configuration_loaded': 'NOT_TESTED', 'cli_mcp_readable': 'NOT_TESTED',
            'agent_auto_invocation': 'NOT_VERIFIED',
            'host_configuration_exists': 'NOT_CHECKED',
            'user_confirmation_receipt_verified': 'NOT_VERIFIED',
            'scope_contract': 'agent-reported-v1',
            'immediate_disable_existing_sessions': 'NOT_VERIFIED',
            'adapter': 'DOCUMENTED_INSTRUCTIONS_ONLY',
            'fallback': 'Use an explicit task prompt and existing authorized MCP configuration; restart host sessions.'}


def rule_plan(root, host, scope, mode):
    if scope not in ('project', 'global') or mode not in ('enabled', 'disabled', 'inherit', 'uninstall'):
        raise ValueError('invalid scope/mode')
    if scope == 'global' and mode == 'inherit':
        raise ValueError('global cannot inherit')
    root = safe(root)
    entry, state = locations(root, host)
    journal = root / '.development-map-install.journal.json'
    if journal.exists():
        raise ValueError('BLOCKED: unresolved journal; recover before planning')
    old = snapshot(entry)
    saved = snapshot(state)
    record = json.loads(saved['content'] or '{}')
    block = managed(old['content'])
    if record:
        if record.get('entry') != str(entry) or record.get('scope') != scope or record.get('host') != host:
            raise ValueError('installation identity conflict')
        if block != record['block']:
            raise ValueError('user modified managed block; resolve manually')
    elif block:
        raise ValueError('unowned managed block; refuse adoption')
    if mode == 'uninstall':
        new_text = old['content']
        if record and record.get('block'):
            # Remove only the exact block we own; preserve outside changes.
            new_text = new_text.replace(record['block'], '', 1)
            if new_text == record['outside_after_append']:
                new_text = record['original']
        new_state = None
    else:
        new_block = rule(mode, scope, root, host)
        text = old['content'] or ''
        if block:
            new_text = text.replace(block, new_block, 1)
        else:
            new_text = text + ('\n' if text and not text.endswith('\n') else '') + new_block
        record = {**record, 'host':host,'scope':scope,'entry':str(entry),'mode':mode,'block':new_block,
                  'original':record.get('original') if block else old['content'],
                  'outside_after_append':record.get('outside_after_append') if block else new_text.replace(new_block,'',1)}
        new_state = canonical(record) + '\n'
    changes=[]
    for path, before, content in [(entry,old,new_text),(state,saved,new_state)]:
        after={'content':content,'mode':before['mode'] if before['mode'] is not None else (0o600 if content is not None else None)}
        if content is None: after['mode']=None
        if before != after:
            changes.append({'path':str(path),'before':before,'after':after})
    result={'version':VERSION,'root':str(root),'host':host,'scope':scope,'mode':mode,
            'changes':changes,'journal':str(journal),'journal_mode':0o600,'temporary_files':str(root / '.devmap-stage-*'),
            'permissions':'No chmod of existing files; new files 0600. No database or credential access.',
            'limits':discover(root,host,scope,local_only=True)}
    result['digest']=digest(canonical(result))
    if any(c['after']['content'] is not None and len(c['after']['content'].encode('utf-8')) > 1024*1024 for c in changes) or len(canonical(result).encode('utf-8')) > 1024*1024:
        raise ValueError('BLOCKED: plan/state/journal exceeds recoverable 1 MiB limit')
    return result



def effective(record, host, scope):
    if not record or record.get('flavor') == 'skill-only':
        return False
    mode = record.get('mode', 'inherit')
    if mode != 'inherit':
        return mode == 'enabled'
    home = record.get('global_root')
    if not home:
        return False
    _, state = locations(Path(home), host)
    global_record = json.loads(read(state) or '{}')
    if global_record and (global_record.get('scope') != 'global' or global_record.get('host') != host):
        raise ValueError('global policy identity conflict')
    global_entry,_=locations(Path(home),host)
    if global_record and managed(read(global_entry))!=global_record.get('block'):
        raise ValueError('global policy file changed; repair before discovery')
    return global_record.get('mode') == 'enabled'


def asset_path(root, relative):
    rel=Path(relative)
    if rel.is_absolute() or '..' in rel.parts or not relative.startswith('.agents/skills/development-map/'):
        raise ValueError('invalid owned asset path')
    return safe(root / rel)


def plan(root, host, scope, mode, flavor=None, global_root=None):
    root=safe(root);entry,state=locations(root,host)
    prior=json.loads(read(state) or '{}')
    flavor=flavor or prior.get('flavor') or ('global' if scope=='global' else 'project')
    if flavor not in ('project','global','skill-only') or (scope=='global') != (flavor=='global'):
        raise ValueError('installation mode and scope mismatch')
    global_root=str(safe(global_root)) if global_root is not None else prior.get('global_root')
    if global_root and (scope!='project' or Path(global_root)==root):
        raise ValueError('global policy root must be separate from project root')
    # Reuse conservative ownership and instruction merging. Skill Only removes a
    # previously owned entry but never modifies unowned instruction text.
    base=rule_plan(root,host,scope,'uninstall' if flavor=='skill-only' else mode)
    changes=[c for c in base['changes'] if c['path']!=str(state)]
    desired={}
    if mode!='uninstall' and flavor!='global':
        source=Path(__file__).resolve().parent/'skills/development-map'
        for f in sorted(source.rglob('*')):
            if f.is_file():
                safe(f)
                rel='.agents/skills/development-map/'+f.relative_to(source).as_posix()
                desired[rel]=read(f)
    if mode != 'uninstall':
        # Install a self-contained status entry in the approved scope, never in
        # a guessed user home. The installed copy refuses lifecycle operations.
        runtime = read(Path(__file__).resolve())
        runtime = "    try:\n        if len(sys.argv) < 2 or sys.argv[1] not in ('status', 'discover', 'verify'):\n            raise ValueError('installed status entry is read-only; use distribution installer for changes')\n        main()".join(runtime.rsplit("    try:main()", 1))
        desired['.agents/skills/development-map/scripts/map-status.py'] = runtime
    owned=prior.get('assets',{})
    for rel in sorted(set(owned)|set(desired)):
        path=asset_path(root,rel);before=snapshot(path)
        if rel in owned:
            if before['content'] is None or digest(before['content'])!=owned[rel]['sha256']:
                raise ValueError('user modified owned Skill asset: '+rel)
        elif before['content'] is not None:
            raise ValueError('unowned Skill asset; explicit migration required: '+rel)
        content=desired.get(rel)
        after={'content':content,'mode':(before['mode'] or 0o600) if content is not None else None}
        if before!=after:changes.append({'path':str(path),'before':before,'after':after})
    if mode=='uninstall':
        new_state=None
    else:
        if flavor=='skill-only':
            record={'host':host,'scope':scope,'entry':str(entry),'mode':mode,'block':None,
                    'original':None,'outside_after_append':None}
        else:
            record=json.loads(next((c['after']['content'] for c in base['changes'] if c['path']==str(state)),read(state)))
        record.update({'installer_version':VERSION,'flavor':flavor,'global_root':global_root,
                       'assets':{rel:{'sha256':digest(value)} for rel,value in desired.items()}})
        new_state=canonical(record)+'\n'
    before=snapshot(state);after={'content':new_state,'mode':(before['mode'] or 0o600) if new_state is not None else None}
    if before!=after:changes.append({'path':str(state),'before':before,'after':after})
    dirs=set()
    for c in changes:
        if c['after']['content'] is not None:
            parent=Path(c['path']).parent
            while not parent.exists():
                safe(parent);dirs.add(str(parent));parent=parent.parent
    created=sorted(dirs,key=lambda x:len(Path(x).parts))
    # Track only directories created by this installation, never claim existing ones.
    tracked=sorted(set(prior.get('directories',[]))|dirs,key=lambda x:len(Path(x).parts))
    for d in tracked:
        safe(Path(d))
        if Path(d) not in [root/'.agents',root/'.agents/skills',root/'.agents/skills/development-map'] and not str(d).startswith(str(root/'.agents/skills/development-map')+'/'):
            raise ValueError('invalid owned directory')
    if new_state is not None:
        record['directories']=tracked
        new_state=canonical(record)+'\n'
        state_change={'path':str(state),'before':before,'after':{'content':new_state,'mode':before['mode'] or 0o600}}
        changes=[c for c in changes if c['path']!=str(state)]
        if state_change['before']!=state_change['after']:changes.append(state_change)
    base.update({'mode':mode,'flavor':flavor,'global_root':global_root,'changes':changes,
                 'create_directories':created,'directory_mode':0o700,
                 'remove_empty_directories':list(reversed(tracked)) if mode=='uninstall' else [],
                 'permissions':'New directories 0700, new files 0600. Preserve existing modes; never open DB or credentials.'})
    base.pop('digest',None);base['digest']=digest(canonical(base))
    if any(c['after']['content'] is not None and len(c['after']['content'].encode())>1024*1024 for c in changes) or len(canonical(base).encode())>1024*1024:
        raise ValueError('BLOCKED: plan/state/journal exceeds recoverable 1 MiB limit')
    return base


def remove_empty(directories):
    for directory in reversed(sorted(directories,key=lambda x:len(Path(x).parts))):
        p=safe(directory)
        if p.is_dir():
            try:
                p.rmdir();sync_dir(p.parent)
            except OSError:
                if not any(p.iterdir()):raise


def sync_dir(path):
    fd=os.open(safe(path),os.O_RDONLY | getattr(os,'O_DIRECTORY',0))
    try:os.fsync(fd)
    finally:os.close(fd)


def put(path, value):
    path=safe(path)
    if value['content'] is None:
        if path.exists():
            path.unlink();sync_dir(path.parent)
        return
    fd,temp=tempfile.mkstemp(prefix='.devmap-stage-',dir=path.parent)
    try:
        os.fchmod(fd,value['mode'])
        with os.fdopen(fd,'wb') as f:
            f.write(value['content'].encode('utf-8'));f.flush();os.fsync(f.fileno())
        safe(path)
        os.replace(temp,path)
        sync_dir(path.parent)
    finally:
        if os.path.exists(temp):os.unlink(temp)


def restore(changes):
    # Validate all before touching any; never overwrite unexpected user edits.
    for c in changes:
        if snapshot(Path(c['path'])) not in (c['before'],c['after']):
            raise ValueError('BLOCKED: rollback conflict; preserve journal and files')
    for c in reversed(changes):put(Path(c['path']),c['before'])


def apply(plan_value, confirm):
    fresh=plan(Path(plan_value['root']),plan_value['host'],plan_value['scope'],plan_value['mode'],plan_value.get('flavor'),plan_value.get('global_root'))
    if fresh != plan_value:
        raise ValueError('plan changed; regenerate and reconfirm')
    if not fresh['changes']: return {'status':'UNCHANGED'}
    # No --yes, persisted approval boolean or source_reference attestation.
    if not confirm(fresh):raise ValueError('user approval required; no changes applied')
    if plan(Path(fresh['root']),fresh['host'],fresh['scope'],fresh['mode'],fresh.get('flavor'),fresh.get('global_root')) != fresh:
        raise ValueError('plan changed during confirmation')
    journal=safe(fresh['journal'])
    fd=os.open(journal,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'w',encoding='utf-8') as f:
        f.write(canonical(fresh));f.flush();os.fsync(f.fileno())
    sync_dir(journal.parent)
    # Shared root journal serializes Codex/DSH writes to the same instructions.
    # A stale plan has written nothing: do not roll it back over another install.
    if any(snapshot(Path(c['path'])) != c['before'] for c in fresh['changes']):
        journal.unlink();sync_dir(journal.parent)
        raise ValueError('concurrent file modification after lock; replan')
    try:
        for directory in fresh.get('create_directories',[]):
            safe(directory).mkdir(mode=0o700)
            sync_dir(Path(directory).parent)
        for c in fresh['changes']:
            if snapshot(Path(c['path'])) != c['before']:raise ValueError('concurrent file modification')
            put(Path(c['path']),c['after'])
        for c in fresh['changes']:
            if snapshot(Path(c['path'])) != c['after']:raise ValueError('verification failed')
    except BaseException:
        restore(fresh['changes'])
        remove_empty(fresh.get('create_directories',[]))
        journal.unlink();sync_dir(journal.parent)
        raise
    remove_empty(fresh.get('remove_empty_directories',[]))
    journal.unlink();sync_dir(journal.parent)
    return {'status':'APPLIED','files_verified':True,'host_loaded':'NOT_VERIFIED','agent_auto_invocation':'NOT_VERIFIED'}


def approve(p):
    print(json.dumps(p,indent=2,ensure_ascii=True))
    # CLI/Agent share planning; an operator must answer on the controlling terminal.
    if not sys.stdin.isatty():return False
    return input('Operator: review exact changes, then type APPROVE '+p['digest']+': ') == 'APPROVE '+p['digest']


def main():
    a=argparse.ArgumentParser(description=__doc__)
    a.add_argument('action',choices=['discover','status','plan','apply','verify','recover','enable','disable','uninstall'])
    a.add_argument('--root',required=True,type=Path,help='explicit existing project root or actual host home')
    a.add_argument('--host',required=True,choices=['codex','dsh','antigravity'])
    a.add_argument('--scope',choices=['project','global'],default='project')
    a.add_argument('--mode',choices=['enabled','disabled','inherit','uninstall'],default='enabled')
    a.add_argument('--plan',type=Path)
    a.add_argument('--flavor',choices=['project','global','skill-only'])
    a.add_argument('--global-root',type=Path,help='explicit actual host home for inherited policy')
    q=a.parse_args()
    if q.action in ('discover','status','verify'):
        v=discover(q.root,q.host,q.scope)
        if q.action=='verify':
            entry,state=locations(q.root,q.host);r=json.loads(read(state) or '{}')
            v['managed_block_intact']=bool(r) and managed(read(entry))==r.get('block')
    elif q.action=='plan':v=plan(q.root,q.host,q.scope,q.mode,q.flavor,q.global_root)
    elif q.action in ('enable','disable','uninstall'):
        mode={'enable':'enabled','disable':'disabled','uninstall':'uninstall'}[q.action]
        v=apply(plan(q.root,q.host,q.scope,mode,q.flavor,q.global_root),approve)
    elif q.action=='apply':
        if not q.plan:raise ValueError('--plan required')
        p=json.loads(read(q.plan))
        if (p['root'],p['host'],p['scope']) != (str(q.root),q.host,q.scope):raise ValueError('plan target mismatch')
        v=apply(p,approve)
    else:
        # Do not trust a journal as arbitrary file-write instructions.
        entry,state=locations(q.root,q.host)
        journal=q.root/'.development-map-install.journal.json';p=json.loads(read(journal))
        unsigned={k:v for k,v in p.items() if k!='digest'}
        if digest(canonical(unsigned)) != p.get('digest'):raise ValueError('journal digest mismatch')
        if p['root'] != str(q.root) or p['host']!=q.host or p['scope']!=q.scope:raise ValueError('journal identity mismatch')
        for c in p['changes']:
            if c['path'] not in (str(entry),str(state)):
                asset_path(q.root,str(Path(c['path']).relative_to(q.root)))
        for directory in p.get('create_directories',[]):
            d=safe(directory)
            if d not in [q.root/'.agents',q.root/'.agents/skills',q.root/'.agents/skills/development-map'] and not str(d).startswith(str(q.root/'.agents/skills/development-map')+'/'):
                raise ValueError('invalid journal directory')
        if not approve(p):raise ValueError('recovery requires user approval')
        restore(p['changes']);remove_empty(p.get('create_directories',[]));journal.unlink();v={'status':'RECOVERED'}
    print(json.dumps(v,indent=2,ensure_ascii=True))


if __name__=='__main__':
    try:main()
    except (ValueError,OSError,KeyError,TypeError) as e:
        print(json.dumps({'status':'BLOCKED','reason':str(e)}),file=sys.stderr);sys.exit(1)

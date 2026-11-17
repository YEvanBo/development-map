#!/usr/bin/env python3
"""Project-scoped setup for signed Development Map bundles. EvanBo, Apache-2.0."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
try:
    import tomllib
except ImportError:
    from vendor import tomli as tomllib

SERVER = 'evanbo_development_map'
START = '<!-- development-map:project-entry -->'
END = '<!-- /development-map:project-entry -->'
ENTRY = (START + '\nFor engineering tasks assigned to this project, use the development-map '
         'Skill and the configured MCP to recover the assigned node before editing. '
         'Ask for a missing project/task binding. Ordinary questions need no map lookup.\n' + END)


def sha(data):
    return hashlib.sha256(data).hexdigest() if data is not None else None


def safe(path):
    if not path.is_absolute() or '..' in path.parts or any(ord(c) < 32 for c in str(path)):
        raise ValueError('use an absolute path without traversal or control characters')
    for item in [*reversed(path.parents), path]:
        try:
            st = item.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(st.st_mode) or getattr(st, 'st_file_attributes', 0) & 1024:
            raise ValueError('link or reparse point: ' + str(item))
    return path


def read(path):
    safe(path)
    if not path.exists():
        return None
    st = path.stat()
    if not stat.S_ISREG(st.st_mode) or st.st_size > 2**20 or st.st_nlink != 1:
        raise ValueError('expected a regular unshared setup file: ' + str(path))
    return path.read_bytes()


def append(text, block):
    newline = '\r\n' if '\r\n' in text else '\n'
    return text + (newline if text and not text.endswith('\n') else '') + block.replace('\n', newline) + newline


def codex_config(old, exe, args):
    text = old.decode('utf-8-sig')
    parsed = tomllib.loads(text)
    table = parsed.get('mcp_servers', {}).get(SERVER)
    header = '[mcp_servers.' + SERVER + ']'
    command = 'command = ' + json.dumps(exe, ensure_ascii=False)
    if table is None:
        updated = append(text, header + '\n' + command + '\nargs = ' + json.dumps(args, ensure_ascii=False))
        assert tomllib.loads(updated)['mcp_servers'][SERVER]['args'] == args
    else:
        if not isinstance(table, dict) or table.get('args') != args or Path(table.get('command','')).name != Path(exe).name:
            raise ValueError('existing Codex server has a different assignment or executable; review it first')
        if table['command'] == exe:
            return old
        # Only update the managed executable. Validate the whole TOML afterwards.
        lines = text.splitlines(keepends=True)
        inside = False
        for i, line in enumerate(lines):
            if line.lstrip().startswith('['):
                inside = line.strip() == header
            elif inside and re.match(r'^\s*command\s*=', line):
                lines[i] = command + ('\r\n' if line.endswith('\r\n') else '\n')
                break
        updated = ''.join(lines)
        table['command'] = exe
        if tomllib.loads(updated) != parsed:
            raise ValueError('custom TOML layout; review the existing command line')
    return (b'\xef\xbb\xbf' if old.startswith(b'\xef\xbb\xbf') else b'') + updated.encode()


def dsh_config(old, exe, args):
    row = {'id':'evanbo-development-map','name':'@deepseek-ai/dsh-mcp-client',
           'config':{'serverName':SERVER,'transport':'stdio','command':exe,'args':args}}
    desired = [{'insert':[row]}]
    if old is not None:
        existing = json.loads(old)
        if existing == desired:
            return old
        previous = existing[0]['insert'][0]['config']['command']
        if Path(previous).name != Path(exe).name:
            raise ValueError('custom DSH command; choose a separate patch')
        existing[0]['insert'][0]['config']['command'] = exe
        if existing != desired:
            raise ValueError('existing DSH patch differs; choose a dedicated patch')
    return (json.dumps(desired, ensure_ascii=False, indent=2)+'\n').encode()


def make_plan(a):
    project = safe(a.project)
    if not project.is_dir():
        raise ValueError('project must already exist')
    system = {'Darwin':'macos','Linux':'linux','Windows':'windows'}.get(platform.system())
    cpu = {'x86_64':'x64','amd64':'x64','aarch64':'arm64','arm64':'arm64'}.get(platform.machine().lower())
    pkg = json.loads((ROOT/'package.json').read_text(encoding='utf-8'))
    if pkg['platform'] != f'{system}-{cpu}':
        raise ValueError('use the package for the target operating system and CPU')
    exe = ROOT/'bin'/('map-mcp.exe' if system=='windows' else 'map-mcp')
    if not exe.is_file() or not (ROOT/'Skill/SKILL.md').is_file():
        raise ValueError('incomplete bundle; verify the signed manifest before setup')
    if a.db == a.token_file:
        raise ValueError('database and token must be distinct files')
    for path in (a.db, a.token_file):
        safe(path)
        if not path.is_file():
            raise ValueError('existing database and provisioned token file required')
    if system!='windows' and a.token_file.stat().st_mode & 0o077:
        raise ValueError('token file must be private (0600)')
    args = [str(a.db),'--token-file',str(a.token_file)]
    if a.host=='codex' and a.dsh_patch:
        raise ValueError('--dsh-patch applies only to DSH')
    cfg = project/'.codex/config.toml' if a.host=='codex' else (a.dsh_patch or project/'.dsh/development-map.patch.json')
    entry = project/('AGENTS.override.md' if a.host=='codex' and (project/'AGENTS.override.md').is_file() else 'AGENTS.md')
    skill = project/'.agents/skills/development-map'
    owned = json.loads((ROOT/'rc4-skill-hashes.json').read_text(encoding='utf-8'))
    assets = {skill/f.relative_to(ROOT/'Skill'): f for f in sorted((ROOT/'Skill').rglob('*')) if f.is_file()}
    paths = [cfg,entry,*assets]
    lock = project/'.development-map-setup.lock'
    forbidden = {a.db,a.token_file,lock}
    if len(set(paths))!=len(paths) or set(paths)&forbidden or lock in (a.db,a.token_file):
        raise ValueError('configuration, program, lock, database and token paths must be distinct')
    for path in paths:
        if path==ROOT or ROOT in path.parents:
            raise ValueError('installation destination inside the program bundle')
    old = {path:read(path) for path in paths}
    proposed = {cfg:(codex_config(old[cfg] or b'',str(exe),args) if a.host=='codex' else dsh_config(old[cfg],str(exe),args))}
    text = (old[entry] or b'').decode('utf-8')
    if '<!-- development-map:discovery:v1 -->' in text:
        raise ValueError('existing managed discovery policy; reconcile using its installer first')
    if START in text or END in text:
        if text.count(START)!=1 or text.count(END)!=1 or ENTRY not in text.replace('\r\n','\n'):
            raise ValueError('project entry was customized; review it first')
        proposed[entry] = old[entry]
    else:
        proposed[entry] = append(text,ENTRY).encode()
    for path, source in assets.items():
        new = source.read_bytes()
        if old[path] not in (None,new) and sha(old[path]) not in owned.get(source.relative_to(ROOT/'Skill').as_posix(),[]):
            raise ValueError('custom Skill file; review before updating: '+str(path))
        proposed[path]=new
    details = [{'path':str(p),'before':sha(old[p]),'after':sha(proposed[p])} for p in paths]
    plan = {'host':a.host,'release':pkg['release'],'project':str(project),'program':str(exe),'database':str(a.db),
            'token_file':str(a.token_file),'configuration':str(cfg),'files':details}
    return old,proposed,plan,sha(json.dumps(plan,sort_keys=True).encode())


def replace(path, data, expected):
    if read(path)!=expected:
        raise ValueError('file changed during setup: '+str(path))
    if data is None:
        path.unlink()
        return
    path.parent.mkdir(parents=True,exist_ok=True)
    mode=stat.S_IMODE(path.stat().st_mode) if path.exists() else 0o600
    with tempfile.NamedTemporaryFile(dir=path.parent,delete=False) as f:
        tmp=Path(f.name)
    try:
        tmp.write_bytes(data)
        tmp.chmod(mode)
        os.replace(tmp,path)
    finally:
        if tmp.exists(): tmp.unlink()


def run(a):
    old,new,plan,plan_id = make_plan(a)
    changes = [p for p in new if old[p]!=new[p]]
    if a.action=='plan':
        print(json.dumps({**plan,'plan_id':plan_id,'changes':[str(p) for p in changes]},ensure_ascii=False,indent=2))
        return
    if a.plan_id!=plan_id:
        raise ValueError('plan changed; run plan again and review the new changes')
    if changes:
        lock=safe(a.project/'.development-map-setup.lock')
        with lock.open('x'):
            pass
        written=[]
        try:
            old,new,_,current=make_plan(a)
            if current!=plan_id: raise ValueError('plan changed while locking')
            for path in changes:
                replace(path,new[path],old[path])
                written.append(path)
        except BaseException:
            for path in reversed(written):
                replace(path,old[path],new[path])
            raise
        finally:
            lock.unlink()
    print(json.dumps({'status':'FILES_INSTALLED','changed':len(changes),'configuration':plan['configuration'],
                      'next':'Open a fresh trusted host session and call map_resume on the authorized project.'}))


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action',choices=['plan','apply'])
    parser.add_argument('--host',choices=['codex','dsh'],required=True)
    parser.add_argument('--project',type=Path,required=True)
    parser.add_argument('--db',type=Path,required=True)
    parser.add_argument('--token-file',type=Path,required=True)
    parser.add_argument('--dsh-patch',type=Path)
    parser.add_argument('--plan-id')
    try:
        run(parser.parse_args())
    except (OSError,ValueError,KeyError,IndexError,TypeError) as err:
        sys.exit('Setup stopped: '+str(err))

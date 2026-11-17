"""Run package installation tests using only disposable projects and fake credentials."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent


class Installation(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix='devmap install ')
        self.addCleanup(tmp.cleanup)
        self.base = Path(tmp.name).resolve()
        self.project = self.base/'project with spaces'
        self.project.mkdir()
        self.db = self.base/'map.sqlite'
        self.db.touch()
        self.token = self.base/'credential'
        self.token.write_text('synthetic fixture')
        self.token.chmod(0o600)
        self.entry = self.project/'AGENTS.md'
        self.entry.write_bytes(b'# Keep existing\r\n')
        self.config = self.project/'.codex/config.toml'
        self.config.parent.mkdir()
        self.config.write_bytes(b'# preserve\r\n[mcp_servers.other]\r\ncommand = "other"\r\n')

    def run_setup(self, action, host='codex', extra=(), success=True):
        cmd=[sys.executable, str(ROOT/'install.py'),action,'--host',host,'--project',str(self.project),
             '--db',str(self.db),'--token-file',str(self.token),*extra]
        p=subprocess.run(cmd,capture_output=True,text=True)
        self.assertEqual(p.returncode==0,success,p.stderr)
        self.assertNotIn('synthetic fixture',p.stdout+p.stderr)
        return json.loads(p.stdout) if success else p.stderr

    def apply_plan(self,host='codex',extra=()):
        plan=self.run_setup('plan',host,extra)
        return self.run_setup('apply',host,(*extra,'--plan-id',plan['plan_id']))

    def test_codex_install_and_repeat(self):
        original=self.config.read_bytes()
        plan=self.run_setup('plan')
        self.assertEqual(self.config.read_bytes(),original)
        self.assertFalse((self.project/'.agents').exists())
        self.assertGreater(len(plan['changes']),2)
        self.run_setup('apply',extra=('--plan-id',plan['plan_id']))
        self.assertTrue(self.config.read_bytes().startswith(original))
        self.assertTrue(self.entry.read_bytes().startswith(b'# Keep existing\r\n'))
        self.assertEqual(self.apply_plan()['changed'],0)

    def test_dsh_profile_and_shared_skill(self):
        self.apply_plan()
        target=self.base/'explicit profile.json'
        self.apply_plan('dsh',('--dsh-patch',str(target)))
        cfg=json.loads(target.read_bytes())[0]['insert'][0]['config']
        self.assertEqual(cfg['args'],[str(self.db),'--token-file',str(self.token)])
        self.assertEqual(self.apply_plan('dsh',('--dsh-patch',str(target)))['changed'],0)

    def test_stale_plan_refused(self):
        first=self.run_setup('plan')
        self.entry.write_text('another user changed this')
        self.run_setup('apply',extra=('--plan-id',first['plan_id']),success=False)
        self.assertFalse((self.project/'.agents').exists())

    def test_modified_skill_refused(self):
        self.apply_plan()
        custom=self.project/'.agents/skills/development-map/SKILL.md'
        custom.write_text('my own changes')
        self.run_setup('plan',success=False)
        self.assertEqual(custom.read_text(),'my own changes')

    def test_upgrade_configuration_only_changes_executable(self):
        self.apply_plan()
        old=self.config.read_text(encoding='utf-8')
        current=str(ROOT/'bin'/('map-mcp.exe' if os.name=='nt' else 'map-mcp'))
        past=str(self.base/'rc4'/Path(current).name)
        edited=old.replace(json.dumps(current,ensure_ascii=False),json.dumps(past,ensure_ascii=False))
        self.assertNotEqual(old,edited)
        self.config.write_text(edited,encoding='utf-8')
        self.assertEqual(self.apply_plan()['changed'],1)
        self.assertEqual(self.config.read_text(encoding='utf-8'),old)

    def test_refuse_other_binding_and_collision(self):
        self.apply_plan()
        self.config.write_text(self.config.read_text().replace('map.sqlite','another.sqlite'))
        self.run_setup('plan',success=False)
        self.run_setup('plan','dsh',('--dsh-patch',str(self.token)),success=False)

    def test_concurrent_install_lock(self):
        plan=self.run_setup('plan')
        lock=self.project/'.development-map-setup.lock'
        lock.touch()
        self.run_setup('apply',extra=('--plan-id',plan['plan_id']),success=False)
        self.assertTrue(lock.exists())
        self.assertFalse((self.project/'.agents').exists())


if __name__=='__main__':
    unittest.main(verbosity=2)

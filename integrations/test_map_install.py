import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('installer',Path(__file__).with_name('map-install.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

class Installer(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name).resolve()
    def plan(self,mode='enabled',host='codex',scope='project'):
        return m.plan(self.root,host,scope,mode)
    def install(self,mode='enabled',host='codex'):
        return m.apply(self.plan(mode,host),lambda p:True)
    def test_attestation_diagnostics_do_not_claim_authorization(self):
        status=m.discover(self.root,"codex","project")
        self.assertEqual(status["user_confirmation_receipt_verified"],"NOT_VERIFIED")
        self.assertEqual(status["scope_contract"],"agent-reported-v1")
        self.assertEqual(status["host_configuration_exists"],"NOT_CHECKED")
        self.assertEqual(status["authorized"],"NOT_CHECKED")

    def test_host_instruction_mechanisms_differ(self):
        override=self.root/'AGENTS.override.md';override.write_text('Codex custom override')
        p=m.plan(self.root,'dsh','project','enabled')
        self.assertIn(str(self.root/'AGENTS.md'),[c['path'] for c in p['changes']])
        self.assertNotIn(str(override),[c['path'] for c in p['changes']])
        m.apply(p,lambda _:True);m.apply(m.plan(self.root,'dsh','project','uninstall'),lambda _:True)
        self.assertEqual(override.read_text(),'Codex custom override')
        p=self.plan();self.assertIn(str(override),[c['path'] for c in p['changes']])
    def test_existing_mcp_config_is_not_overwritten(self):
        codex=self.root/'.codex';codex.mkdir();toml=codex/'config.toml';toml.write_text('[mcp_servers.user]\ncommand="custom"\n')
        dsh=self.root/'dsh-profile.yml';dsh.write_text('composition: []\n')
        before=[toml.read_bytes(),dsh.read_bytes()]
        self.install();self.install('disabled');self.install('uninstall')
        self.assertEqual([toml.read_bytes(),dsh.read_bytes()],before)
    def test_skill_only_to_project_preserves_exact_original(self):
        f=self.root/'AGENTS.md';original=b'USER NO NEWLINE';f.write_bytes(original)
        m.apply(m.plan(self.root,'codex','project','enabled',flavor='skill-only'),lambda _:True)
        m.apply(m.plan(self.root,'codex','project','enabled',flavor='project'),lambda _:True)
        self.install('uninstall');self.assertEqual(f.read_bytes(),original)
    def test_missing_asset_disables_effective_discovery(self):
        self.install();(self.root/'.agents/skills/development-map/SKILL.md').unlink()
        s=m.discover(self.root,'codex','project')
        self.assertFalse(s['installed']);self.assertFalse(s['effective_enabled']);self.assertFalse(s['files_verified'])
    def test_project_installs_skill_and_tracks_upgrade(self):
        self.install()
        skill=self.root/'.agents/skills/development-map/SKILL.md'
        self.assertTrue(skill.is_file())
        self.assertTrue(m.discover(self.root,'codex','project')['installed'])
        with patch.object(m,'VERSION',99):self.install()
        state=json.loads((self.root/'.development-map-install-codex.json').read_text())
        self.assertEqual(state['installer_version'],99)
        skill.write_text(skill.read_text()+'user customization')
        with self.assertRaises(ValueError):self.install()
    def test_skill_only_leaves_instructions_untouched(self):
        entry=self.root/'AGENTS.md';entry.write_text('user instructions')
        p=m.plan(self.root,'codex','project','enabled',flavor='skill-only')
        m.apply(p,lambda _:True)
        self.assertEqual(entry.read_text(),'user instructions')
        self.assertTrue((self.root/'.agents/skills/development-map/SKILL.md').exists())
        self.assertFalse(m.discover(self.root,'codex','project')['effective_enabled'])
        m.apply(m.plan(self.root,'codex','project','uninstall'),lambda _:True)
        self.assertEqual(entry.read_text(),'user instructions')
    def test_global_project_truth_table(self):
        global_root=self.root/'host-home';global_root.mkdir()
        project=self.root/'project';project.mkdir()
        for host in ['codex','dsh']:
            for g in ['enabled','disabled']:
                m.apply(m.plan(global_root,host,'global',g),lambda _:True)
                for override,expected in [('enabled',True),('disabled',False),('inherit',g=='enabled')]:
                    m.apply(m.plan(project,host,'project',override,global_root=global_root),lambda _:True)
                    status=m.discover(project,host,'project')
                    self.assertEqual(status['effective_enabled'],expected)
                    self.assertEqual(status['authorized'],'NOT_CHECKED')
                m.apply(m.plan(project,host,'project','uninstall'),lambda _:True)
            m.apply(m.plan(global_root,host,'global','uninstall'),lambda _:True)
    def test_database_and_identity_fixture_never_touched(self):
        p=self.root/'map.sqlite';p.write_bytes(b'synthetic facts and checkpoint')
        self.install();self.install('disabled');self.install('enabled');self.install('uninstall')
        self.assertEqual(p.read_bytes(),b'synthetic facts and checkpoint')
    def test_cli_noninteractive_cannot_approve(self):
        import subprocess,sys
        p=self.plan();f=self.root/'plan.json';f.write_text(json.dumps(p))
        out=subprocess.run([sys.executable,str(Path(m.__file__)), 'apply','--root',str(self.root),'--host','codex','--plan',str(f)],input='APPROVE '+p['digest'],text=True,capture_output=True)
        self.assertNotEqual(out.returncode,0);self.assertFalse((self.root/'AGENTS.md').exists())
    def test_repeat_and_uninstall(self):
        self.install();self.assertEqual(self.install()['status'],'UNCHANGED');self.install('uninstall');self.assertFalse((self.root/'AGENTS.md').exists());self.assertEqual(self.install('uninstall')['status'],'UNCHANGED')
    def test_preserves_original_bytes(self):
        p=self.root/'AGENTS.md';original=b'User\r\ncustom\r\n';p.write_bytes(original);self.install();self.install('uninstall');self.assertEqual(p.read_bytes(),original)
    def test_outside_edits_preserved(self):
        p=self.root/'AGENTS.md';p.write_text('user rules\n');self.install();p.write_text(p.read_text()+'\nnew user rule\n');self.install('uninstall');self.assertIn('new user rule',p.read_text());self.assertIn('user rules',p.read_text())
    def test_edited_managed_block_blocks_upgrade(self):
        self.install();p=self.root/'AGENTS.md';p.write_text(p.read_text().replace('ordinary questions','every question'));before=p.read_bytes()
        with self.assertRaises(ValueError):self.install('disabled')
        self.assertEqual(p.read_bytes(),before)
    def test_plan_changed_before_apply(self):
        p=self.plan();(self.root/'AGENTS.md').write_text('user edit')
        with self.assertRaises(ValueError):m.apply(p,lambda _:True)
    def test_confirmation_rejected(self):
        with self.assertRaises(ValueError):m.apply(self.plan(),lambda _:False)
        self.assertEqual(list(self.root.iterdir()),[])
    def test_confirmation_race(self):
        def confirm(p):(self.root/'AGENTS.md').write_text('racing edit');return True
        with self.assertRaises(ValueError):m.apply(self.plan(),confirm)
        self.assertEqual((self.root/'AGENTS.md').read_text(),'racing edit')
    def test_large_state_and_journal_rejected_before_write(self):
        p=self.root/'AGENTS.md'
        for text in ['x'*600000,'\U0001f680'*60000]:
            p.write_text(text);before=p.read_bytes()
            with self.assertRaises(ValueError):self.plan()
            self.assertEqual(p.read_bytes(),before)
            self.assertEqual(len(list(self.root.iterdir())),1)
    def test_reversed_markers(self):
        (self.root/'AGENTS.md').write_text(m.END+m.START)
        with self.assertRaises(ValueError):self.plan()
    def test_other_host_cannot_enter_transaction(self):
        plan=self.plan();other=self.plan(host='dsh');original=m.put
        def attempt(path,value):
            if path.name=='AGENTS.md':
                with self.assertRaises(ValueError):m.apply(other,lambda _:True)
            original(path,value)
        with patch.object(m,'put',attempt):m.apply(plan,lambda _:True)
        self.assertFalse((self.root/'.development-map-install-dsh.json').exists())
    def test_duplicate_markers(self):
        (self.root/'AGENTS.md').write_text(m.START+m.END+m.START+m.END)
        with self.assertRaises(ValueError):self.plan()
    def test_symlink_and_hardlink(self):
        p=self.root/'outside';p.write_text('keep');(self.root/'AGENTS.md').symlink_to(p)
        with self.assertRaises(ValueError):self.plan()
        (self.root/'AGENTS.md').unlink();import os;os.link(p,self.root/'AGENTS.md')
        with self.assertRaises(ValueError):self.plan()
    def test_parent_symlink(self):
        p=self.root/'real';p.mkdir();l=self.root/'alias';l.symlink_to(p,target_is_directory=True)
        with self.assertRaises(ValueError):m.plan(l,'codex','project','enabled')
    def test_failure_rolls_back(self):
        original=m.put;count=0
        def fail(path,value):
            nonlocal count
            count+=1
            if count==2:raise OSError('injected failure')
            original(path,value)
        with patch.object(m,'put',fail):
            with self.assertRaises(OSError):self.install()
        self.assertEqual(list(self.root.iterdir()),[])
    def test_rollback_conflict_preserves_journal(self):
        original=m.put;count=0
        def fail(path,value):
            nonlocal count
            count+=1
            if count==2:
                (self.root/'AGENTS.md').write_text('user intervened');raise OSError('failure')
            original(path,value)
        with patch.object(m,'put',fail):
            with self.assertRaises(ValueError):self.install()
        self.assertEqual((self.root/'AGENTS.md').read_text(),'user intervened')
        self.assertTrue(list(self.root.glob('*.journal.json')))
    def test_modes_and_no_authorization(self):
        for mode in ['enabled','disabled','inherit']:
            self.install(mode);s=m.discover(self.root,'codex','project');self.assertEqual(s['enabled'],mode);self.assertEqual(s['authorized'],'NOT_CHECKED');self.assertEqual(s['agent_auto_invocation'],'NOT_VERIFIED')
    def test_host_override_and_scope(self):
        (self.root/'AGENTS.override.md').write_text('custom override')
        p=self.plan();self.assertEqual(p['changes'][0]['path'],str(self.root/'AGENTS.override.md'))
        self.install();
        with self.assertRaises(ValueError):self.plan(scope='global')
    def test_other_hosts(self):
        self.install(host='dsh');self.assertEqual(m.discover(self.root,'dsh','project')['host_configuration_loaded'],'NOT_TESTED')
        with self.assertRaises(ValueError):m.plan(self.root,'antigravity','project','enabled')
    def test_no_arbitrary_plan_edits(self):
        p=self.plan();p['changes'][0]['after']['content']='unapproved'
        with self.assertRaises(ValueError):m.apply(p,lambda _:True)
    def test_permissions_unchanged(self):
        p=self.root/'AGENTS.md';p.write_text('old');p.chmod(0o640);self.install();self.assertEqual(p.stat().st_mode&0o777,0o640);self.install('uninstall');self.assertEqual(p.stat().st_mode&0o777,0o640)
    def test_unresolved_journal_blocks(self):
        (self.root/'.development-map-install.journal.json').write_text('{}')
        with self.assertRaises(ValueError):self.plan()


    def test_broken_global_allows_local_disable_and_uninstall(self):
        for mode in ('disabled','uninstall'):
            with tempfile.TemporaryDirectory() as d:
                home=Path(d).resolve();project=home/'project';project.mkdir()
                m.apply(m.plan(home,'codex','global','enabled'),lambda p:True)
                m.apply(m.plan(project,'codex','project','inherit',global_root=home),lambda p:True)
                (home/'AGENTS.md').unlink()
                with self.assertRaises(ValueError):m.discover(project,'codex','project')
                m.apply(m.plan(project,'codex','project',mode),lambda p:True)
                self.assertFalse(m.discover(project,'codex','project')['effective_enabled'])

    def test_rule_status_survives_distribution_removal(self):
        import shutil,subprocess,re
        with tempfile.TemporaryDirectory() as d:
            root=Path(d).resolve();dist=root/'dist';dist.mkdir();project=root/'project';project.mkdir()
            shutil.copy(Path(m.__file__),dist/'map-install.py')
            shutil.copytree(Path(m.__file__).parent/'skills',dist/'skills')
            spec=importlib.util.spec_from_file_location('relocated',dist/'map-install.py')
            installer=importlib.util.module_from_spec(spec);spec.loader.exec_module(installer)
            installer.apply(installer.plan(project,'codex','project','enabled'),lambda p:True)
            rule=(project/'AGENTS.md').read_text()
            command=re.search(r'run `([^`]+)`',rule).group(1)
            shutil.rmtree(dist)
            result=subprocess.run(command,shell=True,cwd=project,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertTrue(json.loads(result.stdout)['effective_enabled'])
            self.assertNotIn(str(dist),rule);self.assertNotIn(str(project),rule)



    def test_status_runtime_is_owned_read_only_and_uninstalled(self):
        import subprocess,sys
        self.install()
        runtime=self.root/'.agents/skills/development-map/scripts/map-status.py'
        result=subprocess.run([sys.executable,str(runtime),'uninstall','--root',str(self.root),'--host','codex'],capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertTrue(m.discover(self.root,'codex','project')['installed'])
        original=runtime.read_bytes();runtime.write_text('tampered')
        self.assertFalse(m.discover(self.root,'codex','project')['installed'])
        with self.assertRaises(ValueError):self.plan('uninstall')
        runtime.write_bytes(original);self.install('uninstall');self.assertFalse(runtime.exists())

if __name__=='__main__':unittest.main()

import copy
import json
import unittest
from workflow import run, OPERATIONS


def reply(value, error=False):
    return {'isError':error,'content':[{'type':'text','text':json.dumps(value)}]}

class Workflow(unittest.TestCase):
    def test_core_capabilities_not_inferred_from_status(self):
        # Deliberately inconsistent fixture verifies no local permission policy.
        value={'project_id':'p','node':{'id':'n','generation':2},'checkpoint':None,
               'takeover':{'action':'confirm_goal','status_code':'CONFIRM_GOAL','confirmation_event':0},
               'capabilities':{'submit':True,'read':True}}
        out=run({'action':'resume','arguments':{'project_id':'p'}},lambda *a:reply(value))
        self.assertEqual(out['capabilities'],value['capabilities'])
        self.assertEqual(out['checkpoint'],None)
    def test_submit_exact_no_preflight_or_retry(self):
        args={'submission':{'submission_key':'exact'}};calls=[]
        def client(name,body):
            calls.append((name,copy.deepcopy(body)))
            return reply({'created':False,'checkpoint_rowid':7,'event_id':8,'payload_digest':'a'*64})
        out=run({'action':'submit','arguments':args},client)
        self.assertEqual(calls,[('map_submit',args)])
        self.assertEqual(out['status'],'REPLAYED');self.assertIsNone(out['capabilities'])
    def test_unavailable_actions_never_call_operator(self):
        for action in set(OPERATIONS)-{'resume','inspect','submit'}:
            out=run({'action':action,'arguments':{}},lambda *a:self.fail('must not call'))
            self.assertEqual(out['status'],'BLOCKED')
    def test_unknown_precondition_not_discarded(self):
        for key in ('goal_revision','expected_version','host_identity','generation'):
            out=run({'action':'submit','arguments':{key:1}},lambda *a:self.fail('must not call'))
            self.assertEqual(out['reason'],'UNSUPPORTED_PRECONDITION_OR_ARGUMENT')
    def test_no_binding_not_inferred_from_forbidden(self):
        for code in ('FORBIDDEN','EXPIRED_OR_REVOKED','CONFLICT','NO_BINDING'):
            out=run({'action':'resume','arguments':{'project_id':'p'}},lambda *a:reply({'status_code':code},True))
            self.assertEqual(out['reason'],code)
            if code!='NO_BINDING':self.assertIsNone(out['required_user_action'])
    def test_errors_do_not_retry(self):
        calls=[]
        def client(*a):calls.append(a);return reply({'status_code':'CONFLICT'},True)
        out=run({'action':'submit','arguments':{'submission':{}}},client)
        self.assertEqual(len(calls),1);self.assertIsNone(out['receipt'])
    def test_invalid_response_fails_closed(self):
        for response in ({}, {'isError':False,'content':[]},reply({'capabilities':{'submit':True}})):
            out=run({'action':'resume','arguments':{}},lambda *a:response)
            self.assertEqual(out['status'],'BLOCKED');self.assertIsNone(out['capabilities'])

    def test_transport_failure_submission_outcome_unknown(self):
        def client(*a):raise OSError('private transport detail')
        out=run({'action':'submit','arguments':{'submission':{}}},client)
        self.assertEqual(out['status'],'OUTCOME_UNKNOWN');self.assertIsNone(out['receipt'])
        self.assertNotIn('private',str(out))
    def test_malformed_core_nested_shapes(self):
        for value in ({'isError':False,'content':[None]}, reply({'capabilities':{},'takeover':{},'node':{},'checkpoint':[]})):
            out=run({'action':'resume','arguments':{}},lambda *a:value)
            self.assertEqual(out['status'],'BLOCKED')

    def test_unparseable_submission_response_remains_unknown(self):
        for value in ({},reply({'created':True})):
            out=run({'action':'submit','arguments':{'submission':{}}},lambda *a:value)
            self.assertEqual(out['status'],'OUTCOME_UNKNOWN');self.assertIsNone(out['receipt'])

    def test_receipt_types_and_digest_are_strict(self):
        good={'created':True,'checkpoint_rowid':7,'event_id':8,'payload_digest':'a'*64}
        for field,values in {'checkpoint_rowid':[True,0,-1,'7',None], 'event_id':[False,0,1.2,None], 'payload_digest':['bad','A'*64,'a'*63,None]}.items():
            for value in values:
                data={**good,field:value}
                out=run({'action':'submit','arguments':{'submission':{}}},lambda *a:reply(data))
                self.assertEqual(out['status'],'OUTCOME_UNKNOWN');self.assertIsNone(out['receipt'])

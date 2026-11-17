"""Thin in-process facade over an already authenticated Core MCP tool client.

No transport, credentials, database access, authority calculation or automatic retry.
Caller supplies call_tool(name, arguments) from its trusted host SDK, not Agent JSON.
"""
import copy
import json
import re

OPERATIONS = ('intake', 'resume', 'inspect', 'submit', 'accept', 'revoke',
              'reopen', 'verify', 'correction.propose', 'correction.apply',
              'correction.history')


def envelope(action):
    return dict(schema_version='devmap-workflow/1', action=action, status='BLOCKED',
                project_id=None, node_id=None, goal_revision=None,
                checkpoint=None, version=None, generation=None,
                capabilities=None, capabilities_source='UNKNOWN',
                required_user_action=None, reason=None, error=None,
                receipt=None, idempotency=None, context=None)


def run(request, call_tool):
    if type(request) is not dict or set(request) != {'action', 'arguments'}:
        raise ValueError('action and arguments required; unknown fields rejected')
    action, args = request['action'], request['arguments']
    if action not in OPERATIONS or type(args) is not dict:
        raise ValueError('invalid workflow request')
    out = envelope(action)
    if action not in ('resume', 'inspect', 'submit'):
        out.update(reason='UNSUPPORTED_BY_FROZEN_CORE_ADAPTER',
                   required_user_action='Use an explicitly authorized operator workflow; no automatic fallback')
        return out
    # Preserve native supported argument semantics. Do not silently discard CAS fields.
    allowed = {'submission'} if action == 'submit' else {
        'project_id', 'node_id', 'node_ref', 'task_id', 'task_kind', 'reference_id',
        'files_after', 'dependencies_after', 'limit', 'expected_source_version'}
    if set(args) - allowed:
        out['reason'] = 'UNSUPPORTED_PRECONDITION_OR_ARGUMENT'
        return out
    # In particular, do not preflight submit using resume: exact replay may remain
    # legal after completion. Core is the only write gate and replay authority.
    if action == 'submit':
        out['status'] = 'OUTCOME_UNKNOWN'
    try:
        reply = call_tool('map_submit' if action == 'submit' else 'map_resume', copy.deepcopy(args))
    except Exception:
        out.update(status='OUTCOME_UNKNOWN' if action == 'submit' else 'UNAVAILABLE',
                   reason='TRANSPORT_FAILURE',
                   required_user_action='Preserve exact request; reconcile through Core, never invent a new submission key')
        return out
    if type(reply) is not dict or type(reply.get('isError')) is not bool:
        out['reason'] = 'INVALID_CORE_RESPONSE'
        return out
    content = reply.get('content')
    if not isinstance(content, list) or len(content) != 1 or type(content[0]) is not dict or content[0].get('type') != 'text':
        out['reason'] = 'INVALID_CORE_RESPONSE'
        return out
    try:
        data = json.loads(content[0]['text'])
        if type(data) is not dict:
            raise ValueError('object required')
    except (ValueError, KeyError, TypeError):
        out['reason'] = 'INVALID_CORE_RESPONSE'
        return out
    if reply['isError']:
        out.update(status='CORE_ERROR', error=data, reason=data.get('status_code', 'UNKNOWN'))
        if out['reason'] in ('NO_BINDING', 'AMBIGUOUS'):
            out['required_user_action'] = 'Choose an authorized existing task or request creation; no automatic binding'
        return out
    if action == 'submit':
        if (type(data.get('created')) is not bool
                or any(type(data.get(k)) is not int or data[k] <= 0 for k in ('checkpoint_rowid', 'event_id'))
                or type(data.get('payload_digest')) is not str
                or re.fullmatch(r'[0-9a-f]{64}', data['payload_digest']) is None):
            out['reason'] = 'INVALID_CORE_RECEIPT'
            return out
        out.update(status='SUBMITTED' if data['created'] else 'REPLAYED', receipt=data,
                   checkpoint=data['checkpoint_rowid'], idempotency={'source':'core', 'created':data['created']})
        # Receipt has no authoritative project/current goal/capabilities; unknown,
        # never copy untrusted request fields into an apparently verified identity.
        return out
    if not isinstance(data.get('capabilities'), dict) or not isinstance(data.get('takeover'), dict) or not isinstance(data.get('node'), dict):
        out['reason'] = 'INVALID_CORE_CONTEXT'
        return out
    if data.get('checkpoint') is not None and type(data['checkpoint']) is not dict:
        out['reason'] = 'INVALID_CORE_CONTEXT'
        return out
    takeover, cp = data['takeover'], data.get('checkpoint') or {}
    out.update(status=takeover.get('status_code', 'UNKNOWN'),
               project_id=data.get('project_id'), node_id=data['node'].get('id'),
               goal_revision=takeover.get('confirmation_event'),
               checkpoint=cp.get('rowid'), version=cp.get('version'),
               generation=data['node'].get('generation'),
               capabilities=copy.deepcopy(data['capabilities']),
               capabilities_source='core.map_resume', context=data,
               reason=takeover.get('action'))
    if takeover.get('action') == 'confirm_goal':
        out['required_user_action'] = 'Confirm the current task goal through the trusted operator/host path'
    elif takeover.get('action') == 'complete':
        out['required_user_action'] = 'None; stop. Reopening requires separate explicit authorization'
    return out

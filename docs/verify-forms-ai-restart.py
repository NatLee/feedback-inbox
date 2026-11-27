#!/usr/bin/env python3
"""Recover deliberately persisted crash states in the owned disposable workflow Compose volume.
This tests a real process restart, not a claim of killing a real SMTP exchange mid-DATA.
"""
import http.cookiejar
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.request

assert os.environ.get('FI_DISPOSABLE_WORKFLOW_TEST') == 'true'
ROOT = Path(__file__).resolve().parent.parent
BASE = 'http://127.0.0.1:18987'
COMPOSE = ['docker','compose','-p','feedback-workflow','-f','docker-compose.yml','-f','docs/compose.forms-ai-test.yml','--profile','mail']
jar = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
def api(path, body=None):
    request = urllib.request.Request(BASE+'/api'+path, data=None if body is None else json.dumps(body).encode(), headers={'Content-Type':'application/json'})
    return json.load(opener.open(request))
api('/auth/login', {'username':'fixture-admin','password':'disposable-workflow-fixture-123'})
assert api('/ai/providers')['allowed_ollama_origins'] == ['http://ai-fixture:11434']
results = json.loads((ROOT/'docs/forms-ai-browser-results.json').read_text())
sid = results['site_id']
# The browser's anonymous iframe submission has completed an analysis, but cannot auto-send.
feedback = api(f'/feedback?site_id={sid}')['items'][0]
fid = feedback['id']
for _ in range(60):
    workflow = api(f'/feedback/{fid}/ai')
    if workflow['runs'] and workflow['runs'][0]['state'] == 'succeeded': break
    time.sleep(.25)
assert workflow['runs'][0]['state'] == 'succeeded' and not workflow['outbound']
run_id = workflow['runs'][0]['id']
def mails():
    return json.load(urllib.request.urlopen('http://127.0.0.1:18025/api/v1/messages'))['total']
before = mails()
subprocess.run(COMPOSE+['stop','feedback-inbox'],cwd=ROOT,check=True,stdout=subprocess.DEVNULL)
script = r'''
import sqlite3,time,json
connection=sqlite3.connect('/data/feedback-inbox.db')
connection.execute('PRAGMA foreign_keys=ON')
fid,sid,run_id=ARGS
with connection:
    run=connection.execute('SELECT job_id,source_hash FROM ai_runs WHERE id=? AND feedback_id=? AND site_id=?',(run_id,fid,sid)).fetchone()
    assert run
    now=int(time.time())
    connection.execute("UPDATE ai_runs SET state='running',output_json=NULL WHERE id=?",(run_id,))
    connection.execute("UPDATE workflow_jobs SET state='running',token='persisted-crash-fixture',attempt=1,lease_until=? WHERE id=?",(now-1,run[0]))
    job=connection.execute("INSERT INTO workflow_jobs(site_id,kind,payload_json,unique_key,state,attempt,token,lease_until,available_at,created_at) VALUES(?,'reply','{}',?,'running',1,'persisted-crash-fixture',?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now'))",(sid,'restart-unknown-'+str(fid),now-1,now)).lastrowid
    outbound=connection.execute("INSERT INTO outbound_replies(feedback_id,site_id,job_id,mode,state,source_hash,recipient,subject,body,content_hash,message_id,idempotency_key,request_hash,created_at) VALUES(?,?,?,'human','sending',?,'restart@example.test','Synthetic interrupted send','Fixture only','fixture',?,'restart-fixture','fixture',strftime('%Y-%m-%dT%H:%M:%SZ','now'))",(fid,sid,job,run[1],'<restart-'+str(fid)+'@example.test>')).lastrowid
    connection.execute('UPDATE workflow_jobs SET payload_json=? WHERE id=?',(json.dumps({'outbound_id':outbound}),job))
print(json.dumps({'analysis_job_id':run[0],'outbound_id':outbound,'reply_job_id':job}))
'''.replace('ARGS',repr((fid,sid,run_id)))
try:
    receipt = json.loads(subprocess.check_output(['docker','run','--rm','--user','10001:10001','-v','feedback-workflow_feedback-inbox-data:/data','python:3.13-slim','python','-c',script],text=True))
finally:
    subprocess.run(COMPOSE+['start','feedback-inbox'],cwd=ROOT,check=True,stdout=subprocess.DEVNULL)
for _ in range(100):
    try:
        workflow=api(f'/feedback/{fid}/ai')
        if workflow['runs'][0]['state']=='succeeded' and workflow['outbound'][0]['state']=='delivery_unknown': break
    except Exception: pass
    time.sleep(.2)
assert workflow['runs'][0]['state']=='succeeded',workflow
assert workflow['outbound'][0]['state']=='delivery_unknown',workflow
jobs=api(f'/sites/{sid}/ai/jobs')['items']
analysis=next(j for j in jobs if j['id']==receipt['analysis_job_id'])
reply=next(j for j in jobs if j['id']==receipt['reply_job_id'])
assert analysis['state']=='succeeded' and analysis['attempt']==2
assert reply['state']=='delivery_unknown' and reply['attempt']==1
assert mails()==before, 'Restart must not create another SMTP message'
receipt.update(feedback_id=fid,analysis_attempts=analysis['attempt'],reply_attempts=reply['attempt'],mail_count_before=before,mail_count_after=mails(),scenario='Persisted fixture crash states, real API process stop/start')
(ROOT/'docs/forms-ai-restart-results.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt))

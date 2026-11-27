#!/usr/bin/env python3
"""Owned development test only: disposable Compose + local model fixture + Mailpit.
Run with docs/compose.forms-ai-test.yml and the existing Playwright test environment.
No live model credentials or external mail service are used.
"""
import asyncio
import base64
import hashlib
import hmac
import json
import os
import re
from pathlib import Path
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from playwright.async_api import async_playwright, expect

BASE = 'http://127.0.0.1:18987'
MAIL = 'http://127.0.0.1:18025'
OUT = Path(__file__).resolve().parent / 'images'
RUN = str(int(time.time()))
PASSWORD = 'disposable-workflow-fixture-123'

async def settle(page):
    await page.evaluate('window.scrollTo(0,0)')
    await page.evaluate('document.fonts.ready')
    await page.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))')
    assert not await page.evaluate('document.documentElement.scrollWidth > innerWidth'), f'Horizontal overflow: {page.url}'

async def main():
    assert os.environ.get('FI_DISPOSABLE_WORKFLOW_TEST') == 'true', 'Explicit disposable fixture flag required'
    OUT.mkdir(exist_ok=True)
    async with async_playwright() as p:
        browser = await p.chromium.launch(channel=os.environ.get('PLAYWRIGHT_CHANNEL', 'chrome'))
        context = await browser.new_context(viewport={'width':1440,'height':1100}, timezone_id='Asia/Taipei')
        async def api(method, path, data=None):
            response = await context.request.fetch(BASE + '/api' + path, method=method, data=data)
            value = await response.json()
            assert response.ok, (method,path,response.status,value)
            return value
        state = await api('GET','/auth/state')
        await api('POST','/auth/setup' if state['setup_required'] else '/auth/login', {'username':'fixture-admin','display_name':'測試工作桌','password':PASSWORD})
        providers = await api('GET','/ai/providers')
        assert providers['allowed_ollama_origins'] == ['http://ai-fixture:11434'] and providers['auto_send_enabled'], 'Not the disposable fixture deployment'
        await api('PATCH','/auth/me',{'display_name':'測試工作桌','email':'staff-'+RUN+'@example.test'})
        user = (await api('GET','/auth/state'))['user']
        site = await api('POST','/sites',{'name':'範例服務｜本機驗證 '+RUN,'url':'http://127.0.0.1:19088','member_ids':[user['id']],'allowed_origins':['http://127.0.0.1:19088'],'embed_origins':['http://127.0.0.1:19088']})
        sid, key = site['id'],site['key']
        form = await api('GET',f'/sites/{sid}/form')
        schema = form['draft']['schema']
        schema['description'] = '這是本機模型與 Mailpit 的驗證範例；所有資料皆為合成。'
        schema['fields'].extend([
            {'id':'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','key':'request_type','label':'詢問類型','type':'select','required':True,'sensitivity':'public','options':[{'value':'general','label':'一般服務詢問'},{'value':'billing','label':'帳務問題'}]},
            {'id':'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','key':'reply_requested','label':'我希望收到 Email 回覆','type':'checkbox','sensitivity':'public'},
            {'id':'cccccccccccccccccccccccccccccccc','key':'member_id','label':'會員編號（自行填寫）','type':'text','sensitivity':'personal','validation':{'max_length':128}},
        ])
        draft = await api('PUT',f'/sites/{sid}/form/draft',{'expected_revision':form['draft']['revision'],'schema':schema})
        version = await api('POST',f'/sites/{sid}/form/publish',{'expected_revision':draft['revision']})
        provider = await api('POST','/ai/providers',{'name':'本機驗證模型 '+RUN,'kind':'ollama','base_url':'http://ai-fixture:11434','model':'fixture-v1','enabled':True})
        tested = await api('POST',f"/ai/providers/{provider['id']}/test",{'expected_revision':provider['revision']})
        assert tested['tested_revision'] == tested['revision']
        knowledge = await api('POST',f'/sites/{sid}/ai/knowledge',{'title':'服務時間與聯絡方式','body':'服務時間為週一至週五 09:00–17:00（台北時間）。團隊會在服務時間內處理一般詢問。'})
        await api('POST',f"/sites/{sid}/ai/knowledge/{knowledge['id']}/approve",{'expected_revision':knowledge['revision']})
        template = await api('POST',f'/sites/{sid}/ai/templates',{'name':'標準收件確認','subject':'[{site_name}] 已收到您的訊息','body':'我們已收到您的訊息（編號 {reference}），工作人員會盡快回覆。','knowledge_ids':[]})
        await api('POST',f"/sites/{sid}/ai/templates/{template['id']}/approve",{'expected_revision':template['revision']})
        cfg = await api('GET',f'/sites/{sid}/ai')
        await api('POST',f'/sites/{sid}/ai/rules',{'name':'一般詢問收件確認','enabled':True,'field_id':'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','equals':'general','category':cfg['settings']['categories'][0],'template_id':template['id']})
        cfg = await api('GET',f'/sites/{sid}/ai')
        settings = cfg['settings']
        settings.update(enabled=True,provider_id=provider['id'],field_ids=[f['id'] for f in cfg['fields'] if f['key'] in ['message','request_type']],generate_on_receipt=False,expected_revision=settings['revision'])
        await api('PUT',f'/sites/{sid}/ai',settings)
        errors=[]
        page=await context.new_page()
        page.on('pageerror',lambda e:errors.append(str(e)))
        # Exercise the actual form builder after API setup of the fixture scenario.
        await page.goto(BASE+f'/sites/{sid}')
        await page.get_by_label('表單標題',exact=True).fill('範例服務｜歡迎留一封來信')
        await page.get_by_role('button',name='發布表單',exact=True).click()
        await expect(page.get_by_text('表單已發布',exact=True)).to_be_visible()
        version=(await api('GET',f'/sites/{sid}/form'))['published']
        assert version['version']==3
        await settle(page)
        await page.screenshot(path=str(OUT/'forms-ai-builder.png'),full_page=True,animations='disabled')
        # Real standalone form: the backend renders the published version.
        await page.goto(BASE+f'/f/{key}')
        await page.get_by_label(re.compile('^姓名')).fill('林小姐（合成資料）')
        human_email='visitor-'+RUN+'@example.test'
        await page.get_by_label(re.compile('^Email')).fill(human_email)
        await page.get_by_label(re.compile('^訊息')).fill('您好，想了解服務時間。請回覆 '+human_email+'，謝謝。')
        await page.get_by_label(re.compile('^詢問類型')).select_option('general')
        await page.get_by_label('我希望收到 Email 回覆',exact=True).check()
        await settle(page)
        await page.screenshot(path=str(OUT/'forms-ai-hosted.png'),full_page=True,animations='disabled')
        await page.get_by_role('button',name='送出',exact=True).click()
        await expect(page.get_by_role('heading',name='已經收到你的訊息')).to_be_visible()
        feedback=(await api('GET',f'/feedback?site_id={sid}'))['items'][0]
        fid=feedback['id']
        await page.goto(BASE+f'/inbox/{fid}')
        await page.get_by_role('button',name='產生 AI 建議',exact=True).click()
        await expect(page.get_by_text('來信者想了解服務資訊，建議依核准資料回覆。',exact=True)).to_be_visible(timeout=15000)
        await expect(page.get_by_label('客戶回覆內文',exact=True)).not_to_be_empty(timeout=15000)
        analyzed=await api('GET',f'/feedback/{fid}/ai')
        assert human_email not in json.dumps(analyzed['runs'][0]['sources']) and '[EMAIL]' in json.dumps(analyzed['runs'][0]['sources'])
        await page.get_by_role('button',name='採用分類建議',exact=True).click()
        assert (await api('GET',f'/feedback/{fid}'))['status']=='new'
        await page.get_by_label('客戶回覆內文',exact=True).fill('您好，\n\n服務時間為週一至週五 09:00–17:00（台北時間）。\n這封回覆已由工作桌人工核對。\n\n謝謝您的來信。')
        await expect(page.get_by_role('button',name='核對並寄出',exact=True)).to_be_disabled()
        await page.get_by_role('button',name='儲存回覆草稿',exact=True).click()
        await expect(page.get_by_role('button',name='核對並寄出',exact=True)).to_be_enabled()
        await page.get_by_role('button',name='核對並寄出',exact=True).click()
        dialog=page.get_by_role('dialog',name='核對這封客戶回信')
        await expect(dialog).to_contain_text(human_email)
        await expect(dialog.get_by_role('button',name='關閉',exact=True)).to_be_focused()
        await page.keyboard.press('Shift+Tab')
        await expect(dialog.get_by_role('button',name='核准並寄出這個版本')).to_be_focused()
        await page.keyboard.press('Tab')
        await expect(dialog.get_by_role('button',name='關閉',exact=True)).to_be_focused()
        await settle(page)
        await page.screenshot(path=str(OUT/'forms-ai-approval.png'),full_page=False,animations='disabled')
        await dialog.get_by_role('button',name='核准並寄出這個版本').click()
        await expect(page.get_by_text('SMTP 已接受',exact=True)).to_be_visible(timeout=15000)
        human=(await api('GET',f'/feedback/{fid}/ai'))['outbound'][0]
        assert human['state']=='sent' and human['mode']=='human'
        await settle(page)
        await page.screenshot(path=str(OUT/'forms-ai-detail.png'),full_page=True,animations='disabled')
        await page.locator('section').filter(has=page.get_by_role('heading',name='這封信，會寄給來信者',exact=True)).screenshot(path=str(OUT/'forms-ai-reply.png'),animations='disabled')
        # Configure future-only automatic templates through the real application UI.
        await page.goto(BASE+f'/sites/{sid}')
        await page.get_by_role('button',name='AI 與回覆',exact=True).click()
        await page.get_by_label('收到新來信後產生建議',exact=False).check()
        await page.get_by_label('啟用未來來信的範本自動回覆',exact=True).check()
        await page.get_by_role('button',name='儲存並啟用未來自動回覆',exact=True).click()
        await expect(page.get_by_text('設定版本',exact=False)).to_contain_text('範本自動寄送啟用')
        await settle(page)
        await page.screenshot(path=str(OUT/'forms-ai-policy.png'),full_page=True,animations='disabled')
        signing=await api('POST',f'/sites/{sid}/identity-key/rotate',{})
        auto_email='member-'+RUN+'@example.test'
        now=int(time.time())
        claims={'v':1,'site_id':sid,'sub':'fixture-member','email':auto_email,'email_verified':True,'iat':now,'exp':now+300,'jti':'fixture-auto-'+RUN}
        encoded=base64.urlsafe_b64encode(json.dumps(claims,separators=(',',':')).encode()).decode().rstrip('=')
        secret=base64.urlsafe_b64decode(signing['secret']+'='*((4-len(signing['secret'])%4)%4))
        signature=base64.urlsafe_b64encode(hmac.new(secret,encoded.encode(),hashlib.sha256).digest()).decode().rstrip('=')
        payload={'_form_version':version['version'],'_submission_id':'fixture-auto-'+RUN,'_identity_token':encoded+'.'+signature,'name':'已驗證會員（合成）','email':auto_email,'message':'想了解一般服務資訊。','request_type':'general','reply_requested':True,'member_id':'fixture-member'}
        response=await context.request.post(BASE+f'/f/{key}',data=payload)
        assert response.ok,await response.text()
        auto_id=(await api('GET',f'/feedback?site_id={sid}'))['items'][0]['id']
        for _ in range(60):
            auto=await api('GET',f'/feedback/{auto_id}/ai')
            if auto['outbound'] and auto['outbound'][0]['state']=='sent':break
            await asyncio.sleep(.25)
        assert auto['outbound'][0]['mode']=='auto_template' and auto['outbound'][0]['state']=='sent',auto
        # Duplicate submission has no additional worker or mail side effects.
        assert (await context.request.post(BASE+f'/f/{key}',data=payload)).ok
        inbox=await (await context.request.get(MAIL+'/api/v1/messages')).json()
        def addressed(address):
            return [m for m in inbox['messages'] if any(to['Address']==address for to in m.get('To',[]))]
        assert len(addressed(human_email))==1 and len(addressed(auto_email))==1,inbox
        assert len(addressed('staff-'+RUN+'@example.test'))==2,inbox
        # Real separate-origin browser: CORS-readable validation and framed completion.
        verification=await api('POST',f'/sites/{sid}/verification-sessions',{})
        test_payload={'_form_version':version['version'],'_test_token':verification['token'],'name':'跨來源測試','email':'cors@example.test','message':'合成安裝測試','request_type':'general','reply_requested':False}
        html='''<!doctype html><meta charset="utf-8"><title>Feedback integration fixture</title><h1>本機跨來源整合測試</h1><button id="invalid">送出缺少必填欄位</button><button id="valid">送出合成測試</button><pre id="result"></pre><iframe title="回饋表單" style="width:100%;height:800px" src="'''+BASE+'/f/'+key+'/embed'+'''"></iframe><script>const endpoint='''+json.dumps(BASE+'/f/'+key)+''';const payload='''+json.dumps(test_payload)+''';async function send(body){try{const r=await fetch(endpoint,{method:'POST',credentials:'omit',headers:{'Content-Type':'application/json','Accept':'application/json'},body:JSON.stringify(body)});document.querySelector('#result').textContent=r.status+' '+JSON.stringify(await r.json());}catch(e){document.querySelector('#result').textContent='blocked';}}document.querySelector('#invalid').onclick=()=>send({_form_version:payload._form_version});document.querySelector('#valid').onclick=()=>send({...payload,_submission_id:crypto.randomUUID()});</script>'''
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*_):pass
            def do_GET(self):
                self.send_response(200);self.send_header('Content-Type','text/html; charset=utf-8');self.end_headers();self.wfile.write(html.encode())
        servers=[]
        for port in (19088,19089):
            server=ThreadingHTTPServer(('127.0.0.1',port),Handler);threading.Thread(target=server.serve_forever,daemon=True).start();servers.append(server)
        integration=await context.new_page();await integration.goto('http://127.0.0.1:19088')
        await integration.locator('#invalid').click();await expect(integration.locator('#result')).to_contain_text('400')
        await integration.locator('#valid').click();await expect(integration.locator('#result')).to_contain_text('200')
        receipt=await api('GET',f"/sites/{sid}/verification-sessions/{verification['id']}");assert receipt['received']==1
        frame=integration.frame_locator('iframe')
        await frame.get_by_label(re.compile('^姓名')).fill('嵌入表單範例 '+RUN)
        await frame.get_by_label(re.compile('^Email')).fill('iframe-'+RUN+'@example.test')
        await frame.get_by_label(re.compile('^訊息')).fill('確認 iframe 完成畫面留在框架裡。')
        await frame.get_by_label(re.compile('^詢問類型')).select_option('general')
        await frame.get_by_role('button',name='送出',exact=True).click()
        await expect(frame.get_by_role('heading',name='已經收到你的訊息')).to_be_visible()
        await integration.screenshot(path=str(OUT/'forms-ai-embed.png'),full_page=True,animations='disabled')
        await integration.goto('http://127.0.0.1:19089');await integration.locator('#valid').click();await expect(integration.locator('#result')).to_have_text('blocked')
        embed_headers=(await context.request.get(BASE+f'/f/{key}/embed')).headers
        assert 'x-frame-options' not in embed_headers and 'http://127.0.0.1:19088' in embed_headers['content-security-policy']
        assert (await context.request.get(BASE+'/api/meta')).headers['x-frame-options']=='DENY'
        await page.goto(BASE+f'/sites/{sid}');await page.get_by_role('button',name='發布與嵌入',exact=True).click();await page.get_by_role('button',name='開始安裝測試',exact=True).click();await page.get_by_role('button',name='從本站送出合成測試',exact=True).click();await expect(page.get_by_text('已收到 1 / 5 則',exact=True)).to_be_visible()
        await settle(page);await page.screenshot(path=str(OUT/'forms-ai-integrations.png'),full_page=True,animations='disabled')
        await page.goto(BASE+'/settings');await page.get_by_role('button',name='管理 AI 供應商',exact=True).click();await expect(page.get_by_text('本機驗證模型 '+RUN,exact=True)).to_be_visible();await settle(page);await page.screenshot(path=str(OUT/'forms-ai-providers.png'),full_page=True,animations='disabled')
        await page.goto(BASE+'/review');await expect(page.get_by_text('嵌入表單範例 '+RUN,exact=True)).to_be_visible();await settle(page);await page.screenshot(path=str(OUT/'forms-ai-review.png'),full_page=True,animations='disabled')
        await page.set_viewport_size({'width':390,'height':844});await page.goto(BASE+f'/inbox/{fid}');await expect(page.get_by_label('客戶回覆內文',exact=True)).to_be_visible();await settle(page);await page.screenshot(path=str(OUT/'forms-ai-mobile.png'),full_page=True,animations='disabled')
        await page.evaluate("localStorage.setItem('fi.theme','dark')");await page.reload();await expect(page.get_by_label('客戶回覆內文',exact=True)).to_be_visible();await settle(page);await page.screenshot(path=str(OUT/'forms-ai-mobile-dark.png'),full_page=True,animations='disabled')
        assert not errors,errors
        for server in servers:server.shutdown()
        result={'site_id':sid,'human_feedback_id':fid,'automatic_feedback_id':auto_id,'human_mail_count':len(addressed(human_email)),'automatic_mail_count':len(addressed(auto_email)),'staff_mail_count_before_iframe':len(addressed('staff-'+RUN+'@example.test')),'browser_errors':errors,'screenshots':'docs/images/forms-ai-*.png','fixture_provider':'Ollama protocol; no real model','mail_transport':'Mailpit only'}
        (OUT.parent/'forms-ai-browser-results.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
        print(json.dumps(result,ensure_ascii=False))
        await browser.close()

if __name__=='__main__':asyncio.run(main())

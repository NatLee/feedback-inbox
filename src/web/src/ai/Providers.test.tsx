import {screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {expect,test} from 'vitest';
import {fakeApi} from '../test/fakeApi';
import {renderApp} from '../test/render';
import {admin,accounts,meta} from '../test/fixtures';
test('provider setup never tests or enables inference merely by opening and leaves retained credentials out of edits',async()=>{
 const p={id:1,name:'Local review',kind:'ollama',base_url:'http://127.0.0.1:11434/',model:'fixture',enabled:false,revision:1,has_credential:true,tested_revision:null,last_test_error:null};
 const calls=fakeApi({'GET /meta':()=>meta,'GET /auth/state':()=>({user:admin,setup_required:false}),'GET /users':()=>accounts,'GET /ai/providers':()=>({items:[p],encryption_ready:true,allowed_ollama_origins:[p.base_url],smtp_ready:true,auto_send_enabled:false}),'PUT /ai/providers/1':()=>({...p,revision:2})});
 renderApp('/settings');await userEvent.click(await screen.findByRole('button',{name:'管理 AI 供應商'}));await userEvent.click(await screen.findByRole('button',{name:'編輯 Local review'}));const model=screen.getByLabelText('模型名稱');await userEvent.clear(model);await userEvent.type(model,'updated-fixture');await userEvent.click(screen.getByRole('button',{name:'儲存供應商'}));
 const write=calls.find(c=>c.method==='PUT'&&c.path==='/ai/providers/1');expect(write?.body).not.toHaveProperty('credential');expect(write?.body).toMatchObject({enabled:false,expected_revision:1});expect(calls.some(c=>c.path.endsWith('/test'))).toBe(false);
});

import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from 'vitest';
import { fakeApi } from '../test/fakeApi';
import { renderApp } from '../test/render';
import { admin, accounts, meta, site } from '../test/fixtures';

const schema = {schema_version:1,title:'官方網站',description:'留下訊息',submit_label:'送出',success_message:'收到',fields:[{id:'00000000000000000000000000000001',key:'message',label:'訊息',type:'textarea',required:true,help_text:'',sensitivity:'public',validation:{max_length:10000},options:[],default_value:null}]};
const version={id:1,site_id:7,version:1,schema,schema_hash:'hash',published_at:'2026-11-28T00:00:00Z',retired_at:null};
test('adds a required member field and publishes the exact edited schema',async()=>{
 let state={published:version,draft:{revision:1,schema},versions:[version]};
 const calls=fakeApi({'GET /meta':()=>meta,'GET /auth/state':()=>({user:admin,setup_required:false}),'GET /sites/7':()=>site,'GET /users':()=>accounts,'GET /sites/7/form':()=>state,
 'PUT /sites/7/form/draft':body=>{const b=body as {schema:typeof schema};state={...state,draft:{revision:2,schema:b.schema}};return state.draft;},
 'POST /sites/7/form/publish':()=>{state={...state,published:{...version,version:2,schema:state.draft.schema}};return state.published;}});
 renderApp('/sites/7');
 await userEvent.click(await screen.findByRole('button',{name:'新增會員編號'}));
 const row=screen.getByRole('group',{name:'欄位 member_id'});
 await userEvent.click(within(row).getByLabelText('必填'));
 await userEvent.click(screen.getByRole('button',{name:'發布表單'}));
 expect(await screen.findByText('已發布 · v2')).toBeInTheDocument();
 const saved=calls.find(c=>c.method==='PUT')?.body as {schema:{fields:{key:string;required:boolean}[]}};
 expect(saved.schema.fields.find(f=>f.key==='member_id')?.required).toBe(true);
 expect(calls.find(c=>c.path.endsWith('/publish'))?.body).toMatchObject({expected_revision:2});
});

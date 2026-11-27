import {useState} from 'react';
import {Plus,FileCheck2} from 'lucide-react';
import {ErrorBanner,Loading} from '../components/ui';
import {useAsync} from '../lib/hooks';
import {aiApi} from './api';
import type {Knowledge,Template} from './types';
import {ActionStatus,AiSection,Field,SaveButton,StateStamp,useAction} from './shared';
type Content=Knowledge|Template;
const ACK_SUBJECT='[{site_name}] 已收到您的訊息';
const ACK_BODY='我們已收到您的訊息（編號 {reference}），工作人員會盡快回覆。';
const label=(item:Content)=>'title' in item?item.title:item.name;
function Editor({siteId,kind,item,knowledge,onSaved,onClose}:{siteId:number;kind:'knowledge'|'templates';item:Content|null;knowledge:Knowledge[];onSaved:()=>Promise<void>;onClose:()=>void}){
 const[title,setTitle]=useState(item?label(item):'');const[subject,setSubject]=useState(item&&'subject'in item?item.subject:'');const[body,setBody]=useState(item?.body||'');const[ids,setIds]=useState<number[]>(item&&'knowledge_ids'in item?item.knowledge_ids:[]);const action=useAction();
 return <form className="ai-editor space-y-4" onSubmit={e=>{e.preventDefault();void action.run(async()=>{await aiApi.saveContent(siteId,kind,item?.id??null,{...(kind==='knowledge'?{title}:{name:title,subject,knowledge_ids:ids}),body,...(item?{expected_revision:item.revision}:{})});await onSaved();onClose();});}}>
 <div className="flex justify-between gap-3"><h3 className="font-semibold">{item?'建立修改版本':'新增內容'}</h3><button className="btn btn-ghost" type="button" onClick={onClose}>收起</button></div>
 <Field label={kind==='knowledge'?'知識標題':'範本名稱'}><input className="input" required maxLength={120} value={title} onChange={e=>setTitle(e.target.value)}/></Field>
 {kind==='templates'&&<><button type="button" className="btn btn-secondary" onClick={()=>{setTitle(title||'收件確認');setSubject(ACK_SUBJECT);setBody(ACK_BODY);setIds([]);}}>填入標準收件確認範本</button><Field label="範本主旨"><input className="input" required maxLength={200} value={subject} onChange={e=>setSubject(e.target.value)}/></Field></>}
 <Field label={kind==='knowledge'?'核准知識內容':'範本內文'} hint={kind==='knowledge'?'提供明確、可核對的事實。模型只能引用已核准的版本。':'只支援 {site_name}、{reference}、{category}；不插入訪客輸入。'}><textarea className="input min-h-44" required maxLength={10000} value={body} onChange={e=>setBody(e.target.value)}/></Field>
 {kind==='templates'&&<fieldset><legend className="label">引用核准知識版本</legend><p className="hint mb-2">標準收件確認可不引用知識；其他範本必須引用最多 5 筆已核准資料。</p>{knowledge.filter(k=>k.state==='approved').map(k=><label key={k.id} className="mb-2 flex items-start gap-2 text-sm"><input type="checkbox" checked={ids.includes(k.id)} onChange={e=>setIds(e.target.checked?[...ids,k.id]:ids.filter(id=>id!==k.id))}/><span>{k.title} <span className="text-xs text-zinc-500">v{k.revision} · #{k.id}</span></span></label>)}{ids.some(id=>!knowledge.some(k=>k.id===id&&k.state==='approved'))&&<button type="button" className="btn btn-secondary" onClick={()=>setIds(ids.filter(id=>knowledge.some(k=>k.id===id&&k.state==='approved')))}>移除已失效引用</button>}</fieldset>}
 {item&&<p className="ai-notice">儲存會建立新版本，原核准立即失效。請重新檢查內容並核准。</p>}<ActionStatus action={action}/><SaveButton busy={action.busy}>儲存未核准版本</SaveButton>
 </form>;
}
export function KnowledgeManager({siteId,admin,kind,onChanged}:{siteId:number;admin:boolean;kind:'knowledge'|'templates';onChanged:()=>Promise<void>}){
 const{data,error,reload}=useAsync<{items:Content[]}>(()=>kind==='knowledge'?aiApi.knowledge(siteId):aiApi.templates(siteId),[siteId,kind]);const references=useAsync(()=>aiApi.knowledge(siteId),[siteId,kind]);const[editing,setEditing]=useState<Content|null|undefined>();const action=useAction();const reloadAll=async()=>{await reload();await references.reload();await onChanged();};
 return <AiSection index={kind==='knowledge'?'02 / 核准資料':'03 / 固定回覆'} title={kind==='knowledge'?'讓回覆有據可依':'核准後，才能交給自動規則'} hint={kind==='knowledge'?'每次修改保留原文與版本。停用或編輯會阻止仍在等待的相關分析與自動回覆。':'模型產生的自由文字始終需要人工審核。自動回覆只使用這裡的固定範本。'} action={<FileCheck2 className="size-5 text-brand-600"/>}>
 {error&&<ErrorBanner message={error}/>} {!data&&!error&&<Loading/>}<div className="space-y-4">{data?.items.map(item=><article className="rounded border border-zinc-200 p-4 dark:border-zinc-700" key={item.id}><div className="flex flex-wrap items-start justify-between gap-2"><div><h3 className="font-semibold">{label(item)}</h3><p className="hint">版本 {item.revision} · 引用 #{item.id}</p></div><StateStamp state={item.state}/></div>{'subject'in item&&<p className="mt-3 font-semibold">{item.subject}</p>}<p className="prose-message mt-3 max-h-64 overflow-auto text-sm">{item.body}</p>{'knowledge_ids'in item&&<p className="hint mt-3">知識引用：{item.knowledge_ids.length?item.knowledge_ids.map(id=>`#${id}`).join('、'):'標準收件確認'}</p>}{admin&&<div className="mt-4 flex flex-wrap gap-2"><button className="btn btn-secondary" onClick={()=>setEditing(item)}>編輯</button>{item.state==='draft'&&<button className="btn btn-primary" disabled={action.busy} onClick={()=>void action.run(async()=>{await aiApi.transition(siteId,kind,item,'approve');await reloadAll();})}>核准此版本</button>}{item.state!=='retired'&&<button className="btn btn-ghost" disabled={action.busy} onClick={()=>{if(confirm('停用此版本會阻止引用它的待寄回覆。確定停用？'))void action.run(async()=>{await aiApi.transition(siteId,kind,item,'retire');await reloadAll();});}}>停用</button>}</div>}</article>)}</div>
 {data?.items.length===0&&<p className="ai-empty">尚無{kind==='knowledge'?'知識資料':'回覆範本'}。先儲存內容，再由管理員明確核准。</p>}
 <div className="mt-5">{admin&&(editing===undefined?<button className="btn btn-primary" onClick={()=>setEditing(null)}><Plus className="size-4"/>新增{kind==='knowledge'?'知識':'範本'}</button>:<Editor key={editing?.id??'new'} siteId={siteId} kind={kind} item={editing} knowledge={references.data?.items||[]} onSaved={reloadAll} onClose={()=>setEditing(undefined)}/>)}</div><ActionStatus action={action}/>
 </AiSection>;
}

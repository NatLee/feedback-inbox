import {useState} from 'react';
import {CheckCircle2,KeyRound,Plus,Radio} from 'lucide-react';
import {ErrorBanner,Loading} from '../components/ui';
import {useAsync} from '../lib/hooks';
import {aiApi} from './api';
import type {Provider,ProviderKind,ProviderList} from './types';
import {ActionStatus,AiSection,Field,SaveButton,useAction} from './shared';
const ORIGINS:Record<ProviderKind,string>={openai:'https://api.openai.com',anthropic:'https://api.anthropic.com',ollama:'http://127.0.0.1:11434'};
const LABELS:Record<ProviderKind,string>={openai:'OpenAI',anthropic:'Claude / Anthropic',ollama:'Ollama'};
function ProviderEditor({provider,environment,onSaved,onClose}:{provider:Provider|null;environment:ProviderList;onSaved:()=>Promise<void>;onClose:()=>void}){
 const[name,setName]=useState(provider?.name||'');const[kind,setKind]=useState<ProviderKind>(provider?.kind||'openai');const[url,setUrl]=useState(provider?.base_url||ORIGINS.openai);const[model,setModel]=useState(provider?.model||'');const[enabled,setEnabled]=useState(provider?.enabled||false);const[secret,setSecret]=useState('');const[clear,setClear]=useState(false);const action=useAction();
 return <form className="ai-editor space-y-4" onSubmit={e=>{e.preventDefault();void action.run(async()=>{await aiApi.saveProvider(provider?.id??null,{name,kind,base_url:url,model,enabled,...(provider?{expected_revision:provider.revision}:{}),...(clear?{credential:''}:secret?{credential:secret}:{})});setSecret('');await onSaved();onClose();});}}>
 <div className="flex justify-between gap-3"><h3 className="font-semibold">{provider?'編輯供應商':'新增供應商'}</h3><button type="button" className="btn btn-ghost" onClick={onClose}>收起</button></div>
 <div className="grid gap-4 sm:grid-cols-2"><Field label="設定名稱"><input className="input" required maxLength={80} value={name} onChange={e=>setName(e.target.value)}/></Field><Field label="供應商"><select className="input" disabled={!!provider} value={kind} onChange={e=>{const k=e.target.value as ProviderKind;setKind(k);setUrl(ORIGINS[k]);setSecret('');}}>{Object.entries(LABELS).map(([k,label])=><option key={k} value={k}>{label}</option>)}</select></Field></div>
 <Field label="模型名稱" hint="輸入這個供應商實際可用的模型；儲存後以合成資料測試結構化回應。"><input className="input font-mono" required maxLength={200} value={model} onChange={e=>setModel(e.target.value)} placeholder={kind==='ollama'?'你的本機模型名稱':'帳戶可用的模型 ID'}/></Field>
 <Field label="服務根網址" hint={kind==='ollama'?`部署允許來源：${environment.allowed_ollama_origins.join('、')}`:'雲端供應商使用固定官方來源。'}><input className="input font-mono" type="url" required readOnly={kind!=='ollama'} value={url} onChange={e=>setUrl(e.target.value)}/></Field>
 <Field label={kind==='openai'?'OpenAI Platform API Key':kind==='anthropic'?'Anthropic API Key':'Ollama 憑證（選填）'} hint={provider?.has_credential?'已儲存加密憑證；留白保留現有值。':'密鑰只送到本站後端加密保存。'}><input className="input" type="password" autoComplete="new-password" value={secret} disabled={clear||!environment.encryption_ready} onChange={e=>setSecret(e.target.value)} placeholder={provider?.has_credential?'已加密儲存，不會顯示原值':''}/></Field>
 {provider?.has_credential&&<label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={clear} onChange={e=>setClear(e.target.checked)}/>移除已儲存憑證</label>}
 <label className="flex items-start gap-2 text-sm"><input type="checkbox" className="mt-1" checked={enabled} onChange={e=>setEnabled(e.target.checked)}/><span>啟用此供應商供網站選用<span className="hint block">仍需通過連線測試，並在各網站明確啟用 AI。</span></span></label>
 <ActionStatus action={action}/><SaveButton busy={action.busy}>儲存供應商</SaveButton>
 </form>;
}
function ProviderManager(){const{data,error,reload}=useAsync(aiApi.providers,[]);const[editing,setEditing]=useState<Provider|null|undefined>();const action=useAction();return <div className="space-y-4">
 {error&&<ErrorBanner message={error}/>} {!data&&!error&&<Loading/>}
 {data&&<>{!data.encryption_ready&&<p className="ai-notice">伺服器尚未設定有效的 FI_AI_SECRET_KEY。請先設定 32 位元組的部署加密金鑰，才能保存 API Key。</p>}<div className="ai-provider-grid">{data.items.map(p=><article className="ai-provider-card" key={p.id}><div className="flex items-start justify-between gap-3"><span className="section-index">{LABELS[p.kind]}</span>{p.tested_revision===p.revision?<span className="ai-stamp"><CheckCircle2 className="size-3"/>已通過測試</span>:<span className="ai-stamp">尚待測試</span>}</div><h3 className="mt-4 text-lg font-semibold">{p.name}</h3><p className="mt-1 break-all font-mono text-xs text-zinc-500">{p.model}</p><p className="mt-4 text-sm">{p.enabled?'可供網站選用':'已停用'} · {p.has_credential?'憑證已加密':'未設定憑證'}</p>{p.last_test_error&&<p className="mt-3 text-sm text-rose-700 dark:text-rose-300">{p.last_test_error}</p>}<div className="mt-4 flex flex-wrap gap-2"><button className="btn btn-secondary" aria-label={`編輯 ${p.name}`} onClick={()=>setEditing(p)}>編輯</button><button className="btn btn-secondary" disabled={action.busy} onClick={()=>void action.run(async()=>{await aiApi.testProvider(p);await reload();})}><Radio className="size-4"/>測試連線</button></div></article>)}</div>
 {data.items.length===0&&<p className="ai-empty">尚未連接模型。加入一個供應商，再為每個網站選擇可送出的欄位。</p>}
 {editing===undefined?<button className="btn btn-primary" onClick={()=>setEditing(null)}><Plus className="size-4"/>新增供應商</button>:<ProviderEditor key={editing?.id??'new'} provider={editing} environment={data} onSaved={reload} onClose={()=>setEditing(undefined)}/>}
 <p className="hint">「測試連線」只傳送合成資料，每次最多 128 個輸出 token。雲端供應商可能依帳戶方案計費。</p><ActionStatus action={action}/></>}
 </div>}
export function Providers(){const[open,setOpen]=useState(false);return <AiSection index="AI / 模型連線" title="讓模型協助整理來信" hint="供應商只負責產生建議；欄位、審核與寄送權限由收件匣管理。" action={<KeyRound className="size-5 text-brand-600"/>}>{open?<ProviderManager/>:<button className="btn btn-secondary" onClick={()=>setOpen(true)}>管理 AI 供應商</button>}</AiSection>}

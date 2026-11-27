import {IdentityIntegration} from "./IdentityIntegration";
import {Check,Copy,ExternalLink,RefreshCw} from 'lucide-react';
import {useState} from 'react';
import {api} from '../api';
import {ErrorBanner,Loading,useToast} from '../components/ui';
import {useApp} from '../context';
import {useAsync,errorText} from '../lib/hooks';
import type {Site} from '../types';
import {formsApi} from './api';
import type {IntegrationFormat} from './types';
export function Integrations({site,admin,onChanged}:{site:Site;admin:boolean;onChanged:(s:Site)=>void}){
 const {meta}=useApp();const toast=useToast();const [format,setFormat]=useState<IntegrationFormat>('html');const [copied,setCopied]=useState(false);
 const {data,error}=useAsync(()=>formsApi.integration(site.id,format),[site.id,site.key,format]);
 const endpoint=`${meta?.public_url||location.origin}/f/${site.key}`;
 const copy=async(text:string)=>{try{await navigator.clipboard.writeText(text);setCopied(true);setTimeout(()=>setCopied(false),1500);}catch{toast('請選取程式碼手動複製','error');}};
 const rotate=async()=>{if(!confirm('換網址會讓舊的分享連結與整合失效。確定更換？'))return;try{onChanged(await api.rotateKey(site.id));}catch(e){toast(errorText(e),'error');}};
 return <div className="space-y-5"><section className="card p-5"><span className="section-index">01 / 分享表單</span><h2 className="mt-2 text-xl font-semibold">一個連結，就能收到來信</h2><p className="mt-2 text-sm text-zinc-500">分享頁會顯示最新已發布的表單。指定版本的程式碼可保留原有欄位。</p><div className="mt-4 flex flex-wrap items-center gap-2"><code className="min-w-0 flex-1 break-all rounded bg-zinc-100 p-3 text-xs dark:bg-zinc-800">{endpoint}</code><button className="btn btn-secondary" onClick={()=>void copy(endpoint)}>{copied?<Check className="size-4"/>:<Copy className="size-4"/>}複製連結</button><a className="btn btn-secondary" href={endpoint} target="_blank" rel="noreferrer noopener"><ExternalLink className="size-4"/>開啟表單</a>{admin&&<button className="btn btn-ghost" onClick={()=>void rotate()}><RefreshCw className="size-4"/>換網址</button>}</div></section>
 <section className="card p-5"><span className="section-index">02 / 整合到網站</span><div className="mt-3 flex flex-wrap gap-2" role="tablist" aria-label="整合方式">{([['iframe','嵌入表單'],['html','HTML'],['fetch','JavaScript'],['curl','curl'],['node','Node 後端']] as const).map(([value,label])=><button role="tab" aria-selected={format===value} key={value} className={`btn ${format===value?'btn-primary':'btn-secondary'}`} onClick={()=>setFormat(value)}>{label}</button>)}</div><p className="my-4 text-sm text-zinc-500">{format==='iframe'?'iframe 由本站送出表單；請先在「通知與負責人」設定允許嵌入的來源。':format==='fetch'?'瀏覽器來源必須在允許清單內。程式碼不攜帶登入 cookie，也不需要 API 金鑰。':format==='node'?'後端呼叫不受瀏覽器 CORS 限制；仍需驗證自己的登入與輸入。隱藏會員編號不代表已驗證身分。':'HTML 表單可以直接送出；完成後前往感謝頁。請以實際欄位值取代範例。'}</p>{site.origin_mode==='legacy_public'&&<p className="mb-4 rounded border border-amber-400 p-3 text-sm">舊版公開模式：目前接受所有瀏覽器來源。可改成精確來源限制。</p>}{error&&<ErrorBanner message={error}/>} {!data&&!error&&<Loading/>}{data&&<><div className="mb-2 flex justify-between text-xs text-zinc-500"><span>已發布版本 {data.version}</span><button className="btn btn-secondary py-1 text-xs" onClick={()=>void copy(data.code)}><Copy className="size-3"/>複製程式碼</button></div><pre className="overflow-x-auto rounded-lg bg-zinc-950 p-4 text-xs leading-6 text-zinc-100">{data.code}</pre></>}</section><IdentityIntegration siteId={site.id} endpoint={endpoint} admin={admin}/></div>;
}

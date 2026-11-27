import {useEffect} from 'react';
import {ErrorBanner,Loading} from '../components/ui';
import {useAsync} from '../lib/hooks';
import type {FeedbackDetail} from '../types';
import {aiApi} from './api';
import {Suggestions} from './Suggestions';
import {ReplyComposer} from './ReplyComposer';
export function FeedbackWorkflow({detail,admin,onClassified}:{detail:FeedbackDetail;admin:boolean;onClassified:()=>Promise<void>}){
 const workflow=useAsync(()=>aiApi.feedback(detail.id),[detail.id]);const context=useAsync(()=>aiApi.context(detail.site_id),[detail.site_id]);const knowledge=useAsync(()=>aiApi.knowledge(detail.site_id),[detail.site_id]);
 const active=workflow.data?.runs.some(r=>['queued','running'].includes(r.state))||workflow.data?.outbound.some(o=>['queued','sending'].includes(o.state));
 useEffect(()=>{if(!active)return;const timer=window.setInterval(()=>{if(document.visibilityState!=='hidden')void workflow.reload();},2500);return()=>window.clearInterval(timer)},[active,workflow.reload]);
 const refresh=async()=>{await workflow.reload();await onClassified();};
 if(detail.is_test)return <p className="ai-notice">這是安裝測試訊息，不執行 AI、不通知團隊，也不寄送客戶回覆。</p>;
 if(detail.status==='spam')return <p className="ai-notice">垃圾訊息不執行分析或對外回覆。先確認來信，再調整處理狀態。</p>;
 return <div>{workflow.error&&<ErrorBanner message={workflow.error}/>} {context.error&&<ErrorBanner message={context.error}/>} {knowledge.error&&<ErrorBanner message={knowledge.error}/>} {(!workflow.data||!context.data)&&!workflow.error&&!context.error&&<Loading/>}{workflow.data&&context.data&&<><Suggestions id={detail.id} data={workflow.data} context={context.data} onChanged={refresh}/><ReplyComposer key={detail.id} feedbackId={detail.id} email={detail.email} draft={workflow.data.draft} outbound={workflow.data.outbound} knowledge={knowledge.data?.items||[]} smtp={context.data.capabilities.smtp_ready} admin={admin} onChanged={workflow.reload}/></>}</div>;
}

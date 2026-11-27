import {request} from '../api';
import type {AiSettings,Provider,ProviderList,SiteAiContext,Knowledge,Template,Rule,FeedbackAi,Draft,DraftInput,Outbound,ReviewItem,Job} from './types';
const site=(id:number)=>`/sites/${id}/ai`;
export const aiApi={
 providers:()=>request<ProviderList>('GET','/ai/providers'),
 saveProvider:(id:number|null,body:unknown)=>request<Provider>(id===null?'POST':'PUT',`/ai/providers${id===null?'':`/${id}`}`,body),
 testProvider:(p:Provider)=>request<Provider>('POST',`/ai/providers/${p.id}/test`,{expected_revision:p.revision}),
 context:(id:number)=>request<SiteAiContext>('GET',site(id)),
 settings:(id:number,s:AiSettings)=>request<AiSettings>('PUT',site(id),{...s,expected_revision:s.revision}),
 knowledge:(id:number)=>request<{items:Knowledge[]}>('GET',`${site(id)}/knowledge`),
 templates:(id:number)=>request<{items:Template[]}>('GET',`${site(id)}/templates`),
 saveContent:(siteId:number,kind:'knowledge'|'templates',id:number|null,body:unknown)=>request<Knowledge|Template>(id===null?'POST':'PUT',`${site(siteId)}/${kind}${id===null?'':`/${id}`}`,body),
 transition:(siteId:number,kind:'knowledge'|'templates',item:Knowledge|Template,action:'approve'|'retire')=>request<Knowledge|Template>('POST',`${site(siteId)}/${kind}/${item.id}/${action}`,{expected_revision:item.revision}),
 rules:(id:number)=>request<{items:Rule[]}>('GET',`${site(id)}/rules`),
 saveRule:(sid:number,id:number|null,body:Omit<Rule,'id'>)=>request<Rule>(id===null?'POST':'PUT',`${site(sid)}/rules${id===null?'':`/${id}`}`,body),
 removeRule:(sid:number,id:number)=>request('DELETE',`${site(sid)}/rules/${id}`),
 dryRun:(sid:number,id:number)=>request<{eligible:boolean;gate:string;explanation:string}>('POST',`${site(sid)}/auto/dry-run`,{feedback_id:id}),
 feedback:(id:number)=>request<FeedbackAi>('GET',`/feedback/${id}/ai`),
 analyze:(id:number)=>request('POST',`/feedback/${id}/ai/analyze`,{}),
 cancelRun:(id:number)=>request('POST',`/ai/runs/${id}/cancel`,{}),
 classify:(id:number,body:unknown)=>request('POST',`/feedback/${id}/ai/classification`,body),
 saveDraft:(id:number,draft:Draft|null,body:DraftInput)=>request<Draft>(draft?'PUT':'POST',`/feedback/${id}/ai/draft`,{...body,...(draft?{expected_revision:draft.revision}:{})}),
 rejectDraft:(id:number,draft:Draft)=>request<Draft>('POST',`/feedback/${id}/ai/draft/reject`,{expected_revision:draft.revision}),
 send:(id:number,draft:Draft,key:string)=>request<Outbound>('POST',`/feedback/${id}/ai/send`,{expected_revision:draft.revision,content_hash:draft.content_hash,idempotency_key:key}),
 cancelOutbound:(id:number)=>request<Outbound>('POST',`/ai/outbound/${id}/cancel`,{}),
 reconcile:(id:number,resolution:string,note:string)=>request<Outbound>('POST',`/ai/outbound/${id}/reconcile`,{resolution,note}),
 review:()=>request<{items:ReviewItem[]}>('GET','/ai/review'),
 jobs:(id:number)=>request<{items:Job[]}>('GET',`${site(id)}/jobs`),
};

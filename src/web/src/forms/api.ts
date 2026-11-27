import {request} from '../api';
import type {FormState,FormDraft,FormSchema,FormVersion,Integration,IntegrationFormat} from './types';
export const formsApi={
 get:(id:number)=>request<FormState>('GET',`/sites/${id}/form`),
 save:(id:number,revision:number,schema:FormSchema)=>request<FormDraft>('PUT',`/sites/${id}/form/draft`,{expected_revision:revision,schema}),
 publish:(id:number,revision:number)=>request<FormVersion>('POST',`/sites/${id}/form/publish`,{expected_revision:revision}),
 retire:(id:number,version:number)=>request('POST',`/sites/${id}/form/versions/${version}/retire`,{}),
 integration:(id:number,format:IntegrationFormat)=>request<Integration>('GET',`/sites/${id}/integrations?format=${format}`),
};

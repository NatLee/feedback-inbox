export type FieldKind='text'|'textarea'|'email'|'number'|'select'|'checkbox'|'hidden';
export interface FormField {id:string;key:string;label:string;type:FieldKind;required:boolean;help_text:string;sensitivity:'public'|'personal'|'sensitive';validation:{max_length?:number|null;min?:number|null;max?:number|null;integer_only?:boolean};options:{value:string;label:string}[];default_value?:string|null}
export interface FormSchema {schema_version:1;title:string;description:string;submit_label:string;success_message:string;fields:FormField[]}
export interface FormVersion {id:number;site_id:number;version:number;schema:FormSchema;schema_hash:string;published_at:string;retired_at:string|null}
export interface FormDraft {revision:number;schema:FormSchema}
export interface FormState {published:FormVersion;draft?:FormDraft;versions?:FormVersion[]}
export type IntegrationFormat='html'|'fetch'|'curl'|'node'|'iframe';
export interface Integration {format:IntegrationFormat;version:number;endpoint:string;code:string}

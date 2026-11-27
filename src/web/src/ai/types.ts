import type {FormField} from '../forms/types';
export type ProviderKind='openai'|'anthropic'|'ollama';
export interface Provider {id:number;name:string;kind:ProviderKind;base_url:string;model:string;enabled:boolean;revision:number;has_credential:boolean;tested_revision:number|null;last_test_error:string|null}
export interface Capabilities {encryption_ready:boolean;smtp_ready:boolean;auto_send_enabled:boolean}
export interface ProviderList extends Capabilities {items:Provider[];allowed_ollama_origins:string[]}
export interface AiSettings {revision:number;enabled:boolean;provider_id:number|null;generate_on_receipt:boolean;field_ids:string[];include_personal:boolean;categories:string[];tags:string[];daily_attempt_limit:number;timeout_seconds:number;max_output_tokens:number;auto_mode:'off'|'auto_template';auto_daily_limit:number;auto_activated_at:number|null}
export interface SiteAiContext {settings:AiSettings;providers:Provider[];fields:FormField[];members:{id:number;display_name:string}[];capabilities:Capabilities}
export interface Knowledge {id:number;revision:number;title:string;body:string;state:'draft'|'approved'|'retired';created_at:string}
export interface Template {id:number;revision:number;name:string;subject:string;body:string;state:'draft'|'approved'|'retired';knowledge_ids:number[];created_at:string}
export interface Rule {id:number;name:string;enabled:boolean;field_id:string;equals:string;category:string;template_id:number}
export interface Analysis {summary:string;language:string;category:string;tags:string[];urgency:string;owner_id:number|null;next_action:string;missing_information:string[];risk_flags:string[];evidence:{field_id:string;start:number;end:number;quote:string}[];draft:{subject:string;body:string;knowledge_ids:number[]}|null}
export interface Run {id:number;state:string;provider_id:number;model:string|null;created_at:string;error_code:string|null;error_message:string|null;output:Analysis|null;sources:{field_id:string;label:string;text:string}[];usage:{input_tokens:number|null;output_tokens:number|null}|null}
export interface Proposal {id:number;revision:number;state:string;category:string;tags:string[];urgency:string;owner_id:number|null}
export interface Draft {id:number;revision:number;state:string;recipient:string;subject:string;body:string;content_hash:string;knowledge_ids:number[];origin:'human'|'ai';created_at:string}
export interface DraftInput {recipient:string;subject:string;body:string;knowledge_ids:number[]}
export interface Outbound {id:number;state:string;mode:'human'|'auto_template';recipient:string;subject:string;body:string;created_at:string;sent_at:string|null;error_code:string|null;message_id:string}
export interface FeedbackAi {runs:Run[];proposal:Proposal|null;draft:Draft|null;outbound:Outbound[];auto_decision:{gate:string;explanation:string}|null}
export interface ReviewItem {feedback_id:number;site_id:number;site_name:string;sender:string;created_at:string;draft_state:string|null;run_state:string|null;outbound_state:string|null;fallback_reason:string|null}
export interface Job {id:number;kind:string;state:string;attempt:number;error_code:string|null;created_at:string}

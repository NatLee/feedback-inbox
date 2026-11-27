#!/usr/bin/env python3
"""Deterministic local Ollama-protocol fixture. No external calls, credentials or real model."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass  # Never log submitted content.

    def do_POST(self):
        if self.path != '/api/chat':
            self.send_error(404)
            return
        size = int(self.headers.get('Content-Length', '0'))
        if size > 131072:
            self.send_error(413)
            return
        request = json.loads(self.rfile.read(size))
        data = json.loads(request['messages'][1]['content'])
        if 'check' in data:
            output = {'ok': True}
        else:
            source = data['fields'][0]
            text = source['text']
            knowledge = data.get('knowledge', [])
            draft = None
            if knowledge:
                draft = {'subject': '回覆您的服務詢問', 'body': '您好，\n\n' + knowledge[0]['body'] + '\n\n謝謝您的來信。', 'knowledge_ids': [knowledge[0]['id']]}
            output = {
                'summary': '來信者想了解服務資訊，建議依核准資料回覆。',
                'language': 'zh-TW', 'category': data['categories'][0], 'tags': [],
                'urgency': 'normal', 'owner_id': None,
                'next_action': '核對營業資訊與收件者後回覆。',
                'missing_information': [], 'risk_flags': [],
                'evidence': [{'field_id': source['field_id'], 'start': 0, 'end': len(text), 'quote': text}],
                'draft': draft,
            }
        body = json.dumps({'model': 'fixture-v1', 'done': True, 'done_reason': 'stop', 'message': {'role': 'assistant', 'content': json.dumps(output, ensure_ascii=False)}, 'prompt_eval_count': 100, 'eval_count': 120}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

ThreadingHTTPServer(('0.0.0.0', 11434), Handler).serve_forever()

//! Trusted review chrome and sandbox annotation SDK for VesperLens.
//!
//! The review controls never execute in the artifact document. The server
//! renders [`render_review_chrome`] as the top-level page and places the
//! reviewed artifact in a sandboxed iframe without `allow-same-origin`.
//! [`inject_review_sdk`] adds only a bounded annotation/message bridge to the
//! reviewed copy; that bridge cannot submit feedback or reach parent DOM.

/// Trusted top-level review client. It owns all feedback submission controls.
pub(crate) const CHROME_SCRIPT: &str = r#"(function(){
  'use strict';
  const root=document.body, token=root.dataset.token, frame=document.getElementById('artifact');
  const notes=document.getElementById('notes'), list=document.getElementById('annotations');
  const status=document.getElementById('status'), failures=document.getElementById('failures');
  const storageKey='vesper-lens:'+token, state=load();
  let annotations=Array.isArray(state.annotations)?state.annotations:[];
  let layoutWarnings=Array.isArray(state.layoutWarnings)?state.layoutWarnings:[];
  let answers=Array.isArray(state.answers)?state.answers:[];
  let expected=Array.isArray(state.expected)?state.expected:[];
  let round=Number(state.round||0), revision=String(state.revision||'');
  let pickMode=false, submitting=false, interviewMode=false;
  notes.value=String(state.notes||'');

  function load(){try{return JSON.parse(sessionStorage.getItem(storageKey)||'{}')}catch(_){return {}}}
  function save(){try{sessionStorage.setItem(storageKey,JSON.stringify({annotations,layoutWarnings,answers,expected,notes:notes.value,round,revision,scrollY:Number(state.scrollY||0),interview:interviewMode}))}catch(_){}}
  function esc(value){const el=document.createElement('span');el.textContent=String(value||'');return el.innerHTML}
  function setStatus(text,kind){status.textContent=text;status.dataset.kind=kind||''}
  function post(message){if(frame.contentWindow)frame.contentWindow.postMessage(message,'*')}

  function renderAnnotations(){
    list.innerHTML='';
    annotations.forEach(function(annotation,index){
      const card=document.createElement('article');card.className='annotation';
      card.innerHTML='<div class="annotation-head"><code>'+esc(annotation.selector)+'</code><button type="button" aria-label="Remove annotation">×</button></div>'+
        '<label>Comment<textarea class="comment" rows="2"></textarea></label>'+
        '<label>Suggested HTML (optional)<textarea class="suggested" rows="2"></textarea></label>';
      card.querySelector('.comment').value=annotation.comment||'';
      card.querySelector('.suggested').value=annotation.suggested_html||'';
      card.querySelector('.comment').addEventListener('input',function(event){annotations[index].comment=event.target.value;save()});
      card.querySelector('.suggested').addEventListener('input',function(event){annotations[index].suggested_html=event.target.value||null;save()});
      card.querySelector('button').addEventListener('click',function(){
        const removed=annotations.splice(index,1)[0];post({type:'vesper:remove-highlight',id:removed.id});save();renderAnnotations();
      });
      list.appendChild(card);
    });
    document.getElementById('annotation-count').textContent=annotations.length?String(annotations.length):'';
  }

  function renderLayoutWarnings(){
    const box=document.getElementById('diagnostics'),items=document.getElementById('diagnostic-items');items.innerHTML='';
    box.hidden=!layoutWarnings.length;
    layoutWarnings.forEach(function(warning,index){
      const label=document.createElement('label');label.className='diagnostic';
      label.innerHTML='<input type="checkbox"> <span><strong>'+esc(warning.message)+'</strong><code>'+esc(warning.selector)+'</code></span>';
      const input=label.querySelector('input');input.checked=Boolean(warning.selected);
      input.addEventListener('change',function(){layoutWarnings[index].selected=input.checked;save()});items.appendChild(label);
    });
  }

  function submit(action,endSession){
    if(submitting)return;
    const missing=expected.filter(function(id){return !answers.some(function(a){return a.question===id&&String(a.value||'').trim()})});
    if(interviewMode&&action!=='reject'&&missing.length){setStatus('Answer the required questions: '+missing.join(', '),'error');return}
    submitting=true;setStatus('Sending feedback…','working');
    const selectedDiagnostics=layoutWarnings.filter(function(warning){return warning.selected}).map(function(warning){return '[Layout: '+warning.rule+'] '+warning.message+' ('+warning.selector+')'}).join('\n');
    const submittedNotes=[notes.value,selectedDiagnostics].filter(Boolean).join('\n');
    fetch('/s/'+token+'/feedback',{method:'POST',headers:{'content-type':'application/json','x-vesper-lens-token':token},
      body:JSON.stringify({action:action,annotations:annotations,notes:submittedNotes,answers:answers,end_session:Boolean(endSession)})})
      .then(function(response){if(!response.ok)throw new Error('HTTP '+response.status);return response.json()})
      .then(function(){setStatus(endSession?'Session ended.':'Feedback delivered. Keep this tab open for the next round.','ok');submitting=false})
      .catch(function(error){submitting=false;setStatus('Could not send feedback: '+error.message,'error')});
  }

  document.getElementById('approve').addEventListener('click',function(){submit('approve',false)});
  document.getElementById('changes').addEventListener('click',function(){submit('modify',false)});
  document.getElementById('reject').addEventListener('click',function(){submit('reject',false)});
  document.getElementById('end').addEventListener('click',function(){submit('reject',true)});
  document.getElementById('annotate').addEventListener('click',function(event){
    pickMode=!pickMode;event.currentTarget.setAttribute('aria-pressed',String(pickMode));
    event.currentTarget.textContent=pickMode?'Interact with page':'Annotate page';
    post({type:'vesper:set-mode',annotating:pickMode});
  });
  document.getElementById('reload').addEventListener('click',function(){post({type:'vesper:request-state'});setTimeout(function(){frame.src=frame.dataset.src+'?r='+Date.now()},30)});
  notes.addEventListener('input',save);

  window.addEventListener('message',function(event){
    if(event.source!==frame.contentWindow||!event.data||typeof event.data.type!=='string')return;
    const message=event.data;
    if(message.type==='vesper:ready'){
      post({type:'vesper:restore',answers:answers,annotations:annotations,scrollY:Number(state.scrollY||0),annotating:pickMode});
    }else if(message.type==='vesper:annotation'&&message.annotation){
      const existing=annotations.findIndex(function(item){return item.id===message.annotation.id});
      if(existing>=0)annotations[existing]=message.annotation;else annotations.push(message.annotation);
      save();renderAnnotations();
    }else if(message.type==='vesper:review-state'){
      answers=Array.isArray(message.answers)?message.answers:[];
      expected=Array.isArray(message.expected)?message.expected:[];
      interviewMode=Boolean(message.interview);
      const approve=document.getElementById('approve'),changes=document.getElementById('changes'),reject=document.getElementById('reject'),annotate=document.getElementById('annotate'),end=document.getElementById('end');
      approve.hidden=interviewMode;approve.disabled=interviewMode;
      changes.textContent=interviewMode?'Send answers':'Send changes';changes.disabled=false;
      reject.textContent=interviewMode?'Cancel':'Reject';reject.disabled=false;
      annotate.hidden=interviewMode;annotate.disabled=interviewMode;
      end.hidden=interviewMode;end.disabled=interviewMode;
      state.scrollY=Number(message.scrollY||0);save();
    }else if(message.type==='vesper:artifact-failure'){
      failures.hidden=false;failures.textContent='Artifact issue: '+String(message.message||'unknown resource failure');
    }else if(message.type==='vesper:layout-diagnostics'){
      const previous=new Map(layoutWarnings.map(function(warning){return [warning.id,Boolean(warning.selected)]}));
      layoutWarnings=(Array.isArray(message.warnings)?message.warnings:[]).map(function(warning){warning.selected=previous.get(warning.id)||false;return warning});
      save();renderLayoutWarnings();
    }
  });

  function poll(){
    fetch('/s/'+token+'/state',{cache:'no-store'}).then(function(response){if(!response.ok)throw new Error();return response.json()}).then(function(next){
      const nextRound=Number(next.round||0), nextRevision=String(next.revision||'');
      if(round&&nextRound!==round)setStatus('The agent started review round '+nextRound+'.','ok');
      if(revision&&nextRevision&&nextRevision!==revision){post({type:'vesper:request-state'});setTimeout(function(){frame.src=frame.dataset.src+'?revision='+encodeURIComponent(nextRevision)},40)}
      round=nextRound;revision=nextRevision;save();
      setTimeout(poll,1000);
    }).catch(function(){setStatus('Review server disconnected. Re-run the review tool to resume.','error');setTimeout(poll,2500)});
  }
  renderAnnotations();renderLayoutWarnings();poll();
})();"#;

/// Sandboxed artifact bridge. It may observe the artifact DOM and send typed
/// messages to its parent, but contains no fetch call and no feedback action.
pub(crate) const ARTIFACT_SDK_SCRIPT: &str = r#"(function(){
  'use strict';
  if(window.__vesperLensSdk)return;window.__vesperLensSdk=true;
  let annotating=false,hovered=null,shadow=null,highlights=new Map();
  function send(message){parent.postMessage(message,'*')}
  function selector(el){
    if(!el||el.nodeType!==1)return '';
    if(el.id)return '#'+CSS.escape(el.id);
    const parts=[];while(el&&el.nodeType===1&&parts.length<10){
      let part=el.localName||'element';const stable=Array.from(el.classList||[]).filter(function(c){return !c.startsWith('vl-')}).slice(0,2);
      if(stable.length)part+='.'+stable.map(CSS.escape).join('.');
      let index=1,sibling=el;while((sibling=sibling.previousElementSibling))if(sibling.localName===el.localName)index++;
      part+=':nth-of-type('+index+')';parts.unshift(part);el=el.parentElement;
    }return parts.join(' > ');
  }
  function nodePath(node,root){const path=[];let current=node;while(current&&current!==root){if(!current.parentNode)break;path.unshift(Array.prototype.indexOf.call(current.parentNode.childNodes,current));current=current.parentNode}return path}
  function boundary(node,offset){const el=node.nodeType===1?node:node.parentElement;return {selector:selector(el),path:nodePath(node,el),offset:Number(offset)||0}}
  function id(){return crypto.randomUUID?crypto.randomUUID():'vl-'+Date.now()+'-'+Math.random().toString(16).slice(2)}
  function mark(el,annotationId){if(!el)return;el.classList.add('vl-highlight');el.dataset.vlAnnotation=annotationId;highlights.set(annotationId,el)}
  function annotationFor(el,range){
    const annotationId=id(),sel=selector(el),text=String((el&&el.textContent)||'').trim().replace(/\s+/g,' ').slice(0,480);
    if(range){const selected=String(range.toString()).trim().replace(/\s+/g,' ').slice(0,480);return {id:annotationId,selector:sel,comment:'',suggested_html:null,target:{type:'text-range',text:selected,selector:sel,start:boundary(range.startContainer,range.startOffset),end:boundary(range.endContainer,range.endOffset)}}}
    return {id:annotationId,selector:sel,comment:'',suggested_html:null,target:{type:'element',selector:sel,tag:(el.localName||''),text:text}};
  }
  function ensureStyle(){
    if(document.getElementById('vesper-lens-sdk-style'))return;
    const style=document.createElement('style');style.id='vesper-lens-sdk-style';style.textContent='.vl-hover{outline:2px dashed #60a5fa!important;outline-offset:2px}.vl-highlight{outline:2px solid #fbbf24!important;outline-offset:2px}';document.head.appendChild(style);
  }
  function onMove(event){if(!annotating)return;if(hovered)hovered.classList.remove('vl-hover');hovered=event.target;if(hovered)hovered.classList.add('vl-hover')}
  function onClick(event){if(!annotating)return;event.preventDefault();event.stopPropagation();const el=event.target;if(!el)return;const annotation=annotationFor(el,null);mark(el,annotation.id);send({type:'vesper:annotation',annotation:annotation})}
  function onSelection(){if(!annotating)return;const selection=getSelection();if(!selection||selection.rangeCount===0||selection.isCollapsed)return;const range=selection.getRangeAt(0);const el=range.commonAncestorContainer.nodeType===1?range.commonAncestorContainer:range.commonAncestorContainer.parentElement;if(!el)return;const annotation=annotationFor(el,range);mark(el,annotation.id);send({type:'vesper:annotation',annotation:annotation});selection.removeAllRanges()}
  function controls(){
    const grouped={},required=new Set();document.querySelectorAll('[data-vesper-question]').forEach(function(control){
      const question=control.dataset.vesperQuestion;if(!question)return;const field=control.closest('[data-vesper-required]');if(field&&field.dataset.vesperRequired==='true')required.add(question);
      const type=String(control.type||'').toLowerCase();if((type==='radio'||type==='checkbox')&&!control.checked)return;
      const value=String(control.value||'').trim();if(!value)return;(grouped[question]||(grouped[question]=[])).push(value);
    });
    return {answers:Object.keys(grouped).map(function(question){return {question:question,value:grouped[question].join(', ')}}),expected:Array.from(required),scrollY:window.scrollY||0,interview:Boolean(document.querySelector('[data-vesper-question]'))};
  }
  function report(){const value=controls();send({type:'vesper:review-state',answers:value.answers,expected:value.expected,scrollY:value.scrollY,interview:value.interview})}
  function restore(message){
    const byQuestion={};(message.answers||[]).forEach(function(answer){byQuestion[answer.question]=String(answer.value||'').split(', ')});
    document.querySelectorAll('[data-vesper-question]').forEach(function(control){const values=byQuestion[control.dataset.vesperQuestion]||[];const type=String(control.type||'').toLowerCase();if(type==='radio'||type==='checkbox')control.checked=values.includes(String(control.value));else if(values.length)control.value=values.join(', ')});
    (message.annotations||[]).forEach(function(annotation){try{mark(document.querySelector(annotation.selector),annotation.id)}catch(_){}});
    window.scrollTo(0,Number(message.scrollY||0));annotating=Boolean(message.annotating);report();
  }
  function auditLayout(){
    const warnings=[],root=document.documentElement;
    if(root.scrollWidth>window.innerWidth+2)warnings.push({id:'horizontal-overflow:document',rule:'horizontal-overflow',selector:'html',message:'Page content extends '+(root.scrollWidth-window.innerWidth)+'px beyond the viewport.'});
    Array.from(document.body?document.body.querySelectorAll('*'):[]).slice(0,800).forEach(function(el){
      if(warnings.length>=20||el.id==='vesper-lens-sdk-style')return;const style=getComputedStyle(el),clips=style.overflow==='hidden'||style.overflow==='clip'||style.overflowX==='hidden'||style.overflowY==='hidden';
      if(!clips||!String(el.textContent||'').trim())return;const horizontal=el.scrollWidth>el.clientWidth+2,vertical=el.scrollHeight>el.clientHeight+2;if(!horizontal&&!vertical)return;
      const sel=selector(el),axis=horizontal&&vertical?'width and height':horizontal?'width':'height';warnings.push({id:'clipped-content:'+sel,rule:'clipped-content',selector:sel,message:'Content appears clipped along its '+axis+'.'});
    });
    send({type:'vesper:layout-diagnostics',warnings:warnings});
  }
  window.addEventListener('message',function(event){const message=event.data||{};if(message.type==='vesper:set-mode')annotating=Boolean(message.annotating);else if(message.type==='vesper:remove-highlight'){const el=highlights.get(message.id);if(el){el.classList.remove('vl-highlight');delete el.dataset.vlAnnotation}highlights.delete(message.id)}else if(message.type==='vesper:request-state')report();else if(message.type==='vesper:restore')restore(message)});
  window.addEventListener('error',function(event){const target=event.target;if(target&&target!==window&&(target.src||target.href))send({type:'vesper:artifact-failure',message:'Could not load '+String(target.src||target.href)})},true);
  document.addEventListener('mousemove',onMove,true);document.addEventListener('click',onClick,true);document.addEventListener('mouseup',onSelection,true);
  document.addEventListener('input',report,true);document.addEventListener('change',report,true);window.addEventListener('scroll',report,{passive:true});
  let auditTimer=0;window.addEventListener('resize',function(){clearTimeout(auditTimer);auditTimer=setTimeout(auditLayout,150)});
  ensureStyle();send({type:'vesper:ready'});report();requestAnimationFrame(function(){requestAnimationFrame(auditLayout)});
})();"#;

/// Render the trusted outer review page.
#[must_use]
pub(crate) fn render_review_chrome(token: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>VesperLens Review</title><style>
*{{box-sizing:border-box}}:root{{color-scheme:dark;font-family:ui-sans-serif,system-ui,sans-serif;background:#0b0f17;color:#e6edf7}}body{{margin:0;height:100vh;overflow:hidden}}.layout{{height:100%;display:grid;grid-template-columns:minmax(0,1fr) 340px}}.frame{{padding:12px;background:#090d14}}iframe{{width:100%;height:100%;border:1px solid #334155;border-radius:10px;background:white}}aside{{overflow:auto;padding:16px;background:#111827;border-left:1px solid #334155}}h1{{font-size:16px;margin:0 0 4px}}h2{{font-size:13px;margin:12px 0 4px}}.subtle,#status{{font-size:12px;color:#94a3b8}}.actions{{display:flex;flex-wrap:wrap;gap:7px;margin:14px 0}}button{{border:1px solid #475569;border-radius:7px;background:#1e293b;color:#e2e8f0;padding:8px 10px;cursor:pointer}}button:hover{{background:#334155}}button:disabled{{cursor:wait;opacity:.55}}#approve{{background:#86efac;color:#052e16}}#changes{{background:#fde68a;color:#422006}}#reject,#end{{background:#fda4af;color:#4c0519}}#annotate[aria-pressed=true]{{background:#60a5fa;color:#082f49}}label{{display:grid;gap:5px;font-size:12px;margin-top:10px}}textarea{{width:100%;resize:vertical;border:1px solid #475569;border-radius:7px;background:#0f172a;color:#e2e8f0;padding:8px;font:inherit}}.annotation{{border:1px solid #334155;border-left:3px solid #fbbf24;border-radius:8px;padding:9px;margin-top:10px;background:#0f172a}}.annotation-head{{display:flex;gap:6px;align-items:start}}.annotation-head code{{font-size:10px;color:#93c5fd;word-break:break-all;flex:1}}.annotation-head button{{padding:1px 7px}}.diagnostic{{grid-template-columns:auto 1fr;align-items:start;border:1px solid #78350f;border-radius:7px;padding:7px;background:#1c1917}}.diagnostic span{{display:grid;gap:3px}}.diagnostic code{{font-size:10px;color:#fbbf24;word-break:break-all}}#status{{min-height:20px;margin-top:8px}}#status[data-kind=error],#failures{{color:#fda4af}}#status[data-kind=ok]{{color:#86efac}}#failures{{font-size:12px;margin:8px 0}}@media(max-width:800px){{.layout{{grid-template-columns:1fr;grid-template-rows:60vh 40vh}}aside{{border-left:0;border-top:1px solid #334155}}}}
</style></head><body data-token="{token}"><div class="layout"><div class="frame"><iframe id="artifact" title="Artifact under review" sandbox="allow-scripts allow-forms allow-popups allow-downloads" data-src="/s/{token}/artifact/index.html" src="/s/{token}/artifact/index.html"></iframe></div><aside><h1>VesperLens Review <span id="annotation-count"></span></h1><div class="subtle">Operate the artifact normally, or enter annotation mode to target changes.</div><div class="actions"><button id="approve" type="button" disabled>Approve</button><button id="changes" type="button" disabled>Send changes</button><button id="reject" type="button" disabled>Reject</button><button id="annotate" type="button" aria-pressed="false" disabled>Annotate page</button><button id="reload" type="button">Reload</button><button id="end" type="button" disabled>End session</button></div><div id="status" role="status" aria-live="polite"></div><div id="failures" hidden></div><section id="diagnostics" hidden><h2>Possible layout issues</h2><div class="subtle">Select only confirmed issues to include in feedback.</div><div id="diagnostic-items"></div></section><div id="annotations"></div><label>Overall notes (optional)<textarea id="notes" rows="4" placeholder="Feedback for the agent"></textarea></label></aside></div><script src="/s/{token}/chrome.js" defer></script></body></html>"#
    )
}

/// Remove artifact-authored CSP meta tags from the isolated review copy so
/// the owned external SDK can run. The trusted outer chrome has its own CSP.
fn strip_csp_meta_tags(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if !lower.contains("content-security-policy") {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find('<') {
        let start = cursor + relative;
        out.push_str(&html[cursor..start]);
        let Some(end_relative) = lower[start..].find('>') else {
            break;
        };
        let end = start + end_relative + 1;
        let tag = &lower[start..end];
        if !(tag.starts_with("<meta")
            && tag.contains("http-equiv")
            && tag.contains("content-security-policy"))
        {
            out.push_str(&html[start..end]);
        }
        cursor = end;
    }
    out.push_str(&html[cursor..]);
    out
}

/// Inject only the sandbox bridge into a reviewed HTML copy.
#[must_use]
pub fn inject_review_sdk(html: &str, sdk_path: &str) -> String {
    let html = strip_csp_meta_tags(html);
    let script = format!(
        "\n<!-- VesperLens sandbox SDK -->\n<script src=\"{}\" defer></script>\n",
        escape_html(sdk_path)
    );
    if let Some(index) = html.to_ascii_lowercase().rfind("</body") {
        let mut output = String::with_capacity(html.len() + script.len());
        output.push_str(&html[..index]);
        output.push_str(&script);
        output.push_str(&html[index..]);
        output
    } else {
        format!("{html}{script}")
    }
}

/// Compatibility name retained for callers; now injects only the isolated SDK.
#[must_use]
pub fn inject_review_overlay(html: &str) -> String {
    inject_review_sdk(html, "/sdk.js")
}

/// Builds a self-contained planning interview artifact.
#[must_use]
pub fn render_interview_artifact(title: &str, questions: &[super::LensQuestion]) -> String {
    let mut fields = String::new();
    for question in questions {
        let id = escape_html(question.id.trim());
        let prompt = escape_html(question.prompt.trim());
        fields.push_str(&format!(
            "<fieldset data-vesper-required=\"{}\"><legend>{prompt}</legend>",
            question.required
        ));
        if !question.description.trim().is_empty() {
            fields.push_str(&format!(
                "<p class=\"description\">{}</p>",
                escape_html(question.description.trim())
            ));
        }
        if !question.recommended.trim().is_empty() {
            fields.push_str(&format!(
                "<p class=\"recommended\">Recommended: {}</p>",
                escape_html(question.recommended.trim())
            ));
        }
        if question.options.is_empty() {
            fields.push_str(&format!("<textarea data-vesper-question=\"{id}\" rows=\"3\" placeholder=\"Type your answer\"></textarea>"));
        } else {
            let input_type = if question.allow_multiple {
                "checkbox"
            } else {
                "radio"
            };
            for option in &question.options {
                let option = escape_html(option);
                fields.push_str(&format!("<label><input type=\"{input_type}\" name=\"{id}\" data-vesper-question=\"{id}\" value=\"{option}\"> <span>{option}</span></label>"));
            }
            if question.allow_other {
                fields.push_str(&format!("<label class=\"other\">Other <input type=\"text\" data-vesper-question=\"{id}\" placeholder=\"Enter another answer\"></label>"));
            }
        }
        fields.push_str("</fieldset>");
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title}</title><style>:root{{color-scheme:dark;font-family:ui-sans-serif,system-ui,sans-serif;background:#0b0f17;color:#e6edf7}}body{{margin:0;padding:42px;max-width:850px}}main{{display:grid;gap:18px}}h1{{margin:0}}header>p,.description{{color:#9aa8ba}}fieldset{{border:1px solid #334155;border-radius:10px;padding:18px;display:grid;gap:10px}}legend{{font-weight:650;padding:0 8px}}label{{display:flex;gap:10px;align-items:flex-start;padding:9px;border-radius:7px;background:#111827}}textarea,input[type=text]{{box-sizing:border-box;width:100%;border:1px solid #475569;border-radius:7px;background:#0f172a;color:#e6edf7;padding:10px;font:inherit}}.recommended{{color:#86efac;margin:0}}.other{{display:grid}}</style></head><body><main><header><h1>{title}</h1><p>Answer the planning questions, add optional context in VesperLens, then send your answers.</p></header>{fields}</main></body></html>"#,
        title = escape_html(title.trim())
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_copy_contains_only_the_sandbox_sdk() {
        let output = inject_review_sdk("<html><body><h1>Hi</h1></body></html>", "/s/token/sdk.js");
        assert!(output.contains("<script src=\"/s/token/sdk.js\" defer>"));
        assert!(!output.contains("Approve"));
        assert!(!ARTIFACT_SDK_SCRIPT.contains("fetch("));
        assert!(!ARTIFACT_SDK_SCRIPT.contains("feedback"));
    }

    #[test]
    fn trusted_chrome_sandboxes_the_artifact_without_same_origin() {
        let html = render_review_chrome("safe-token");
        assert!(
            html.contains("sandbox=\"allow-scripts allow-forms allow-popups allow-downloads\"")
        );
        assert!(!html.contains("allow-same-origin"));
        assert!(html.contains("/s/safe-token/chrome.js"));
    }

    #[test]
    fn chrome_owns_submission_and_requires_the_session_header() {
        assert!(CHROME_SCRIPT.contains("x-vesper-lens-token"));
        assert!(CHROME_SCRIPT.contains("/feedback"));
        assert!(!ARTIFACT_SDK_SCRIPT.contains("/feedback"));
    }

    #[test]
    fn csp_is_removed_only_from_the_isolated_copy() {
        let output = inject_review_sdk(
            "<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'\"><meta name=\"x\" content=\"y\"></head></html>",
            "/sdk.js",
        );
        assert!(
            !output
                .to_ascii_lowercase()
                .contains("content-security-policy")
        );
        assert!(output.contains("name=\"x\""));
    }

    #[test]
    fn rich_interview_metadata_is_escaped_and_rendered() {
        let html = render_interview_artifact(
            "Plan <review>",
            &[super::super::LensQuestion {
                id: "scope".into(),
                prompt: "What ships?".into(),
                description: "Choose <carefully>".into(),
                options: vec!["Web".into()],
                allow_multiple: false,
                required: false,
                recommended: "Web".into(),
                allow_other: true,
            }],
        );
        assert!(html.contains("Plan &lt;review&gt;"));
        assert!(html.contains("Choose &lt;carefully&gt;"));
        assert!(html.contains("data-vesper-required=\"false\""));
        assert!(html.contains("Recommended: Web"));
        assert!(html.contains("Other"));
    }

    #[test]
    fn precise_ranges_and_editable_annotations_are_supported() {
        assert!(ARTIFACT_SDK_SCRIPT.contains("text-range"));
        assert!(ARTIFACT_SDK_SCRIPT.contains("start:boundary"));
        assert!(CHROME_SCRIPT.contains("suggested_html"));
        assert!(CHROME_SCRIPT.contains("vesper:remove-highlight"));
    }

    #[test]
    fn layout_diagnostics_are_passive_and_reviewer_selected() {
        assert!(ARTIFACT_SDK_SCRIPT.contains("horizontal-overflow"));
        assert!(ARTIFACT_SDK_SCRIPT.contains("clipped-content"));
        assert!(CHROME_SCRIPT.contains("warning.selected"));
        assert!(!ARTIFACT_SDK_SCRIPT.contains("/feedback"));
    }

    #[test]
    fn target_text_has_a_bound() {
        assert!(ARTIFACT_SDK_SCRIPT.contains("slice(0,480)"));
    }
}

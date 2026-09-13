"""Reproduce independently authored, frozen semantic-routing evaluation data.

No router, previous corpus, prediction, or skill body is an input.
"""
import collections
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CATALOG = Path('/tmp/routing-bounded-metadata.json')
catalog_bytes = CATALOG.read_bytes()
catalog = json.loads(catalog_bytes)
known = {s['slug'] for s in catalog}
cases = []

def add(kind, prompt, skills=(), rationale='', *, clarity='clear', abstain=False,
        forbidden=(), unavailable=(), denied=()):
    cases.append(dict(
        id=f'independent-{len(cases)+1:03d}', split='independent_holdout',
        kind=kind, clarity=clarity, prompt=prompt,
        expected_skill_ids=list(skills), acceptable_abstention=abstain,
        forbidden_skill_ids=list(forbidden),
        context=dict(unavailable_tools=list(unavailable), denied_permissions=list(denied)),
        rationale=rationale,
    ))

# Tasks were written from catalog capability meanings, not expanded descriptor templates.
positive = [
('youtube-content', 'The attached transcript is from our channel interview. Turn it into a useful episode recap and a short social thread.', 'Transforms a video transcript into publication formats.'),
('xurl', 'Use my connected X account to look up the original posts mentioning our launch yesterday.', 'Direct post retrieval on X.'),
('xlsx', 'In budget.xlsx, preserve the formulas but add a worksheet comparing actual spending with the plan.', 'Editing a real Excel workbook.'),
('work-unit-reporting', 'Package the finished repair evidence into our formal execution record and give me the complete delivery summary in chat.', 'Explicit dual-artifact completion reporting.'),
('weights-and-biases', 'Our training runs are scattered. Put their metrics into W&B and compare the learning-rate sweep in a dashboard.', 'Experiment tracking in the named platform.'),
('weekly-review-planning', 'Friday reset: review my commitments, find what stalled, and make a realistic schedule for next week.', 'Weekly commitment review, not software implementation planning.'),
('vesper-skill-authoring', 'Create a reusable Agent Vesper skill for our release checklist with a valid header and explicit tool bounds.', 'Skill artifact authoring for Vesper.'),
('verify-with-xtask-verify', 'Verify this Agent Vesper Rust patch using the canonical local gate, then check evidence for all five platform targets.', 'Repository-specific canonical verification.'),
('verify-parity-end-to-end', 'Before declaring /skills fixed, exercise the shipped ACP and terminal clients and compare what users actually see.', 'Shipped cross-host command parity.'),
('touchdesigner-mcp', 'In the open TouchDesigner project, use the twozero connection to change the noise operator and check the result.', 'Control of a named application through its catalog integration.'),
('test-driven-development', 'Implement the password validator test-first: demonstrate the missing behavior fails, add the minimum code, then clean it up.', 'Explicit red-green-refactor request.'),
('teams-meeting-pipeline', 'Retrieve yesterday’s Teams meeting transcript through our Microsoft account and extract the follow-ups.', 'Teams source acquisition and transcript pipeline.'),
('systematic-debugging', 'The service drops every third request after reconnecting. Reproduce it and establish the cause before changing code.', 'Root-cause investigation before repair.'),
('subprocess-kill-in-agent-tools', 'The Vesper tool executor kills a shell but hangs reading output because its grandchildren keep the pipe open. Fix cancellation without unsafe Rust.', 'Specific descendant-process pipe lifetime defect.'),
('spike', 'Before we commit to this parser design, build a disposable experiment to see whether it handles a million events per second.', 'Throwaway feasibility experiment.'),
('songwriting-and-ai-music', 'Write a hopeful chorus about moving away from home, then give me a Suno prompt for a gentle indie arrangement.', 'Lyrics and AI music prompt craft.'),
('songsee', 'Generate mel and chroma plots from rehearsal.wav so I can compare the two takes visually.', 'Audio feature visualization.'),
('sketch', 'I need three quick disposable HTML directions for this signup screen so the team can choose a layout.', 'Multiple throwaway UI variants.'),
('simplify-code', 'Have four parallel reviewers simplify our recent code changes and reconcile their cleanup recommendations.', 'Explicit four-agent recent-change cleanup.'),
('session-librarian', 'Find the old coding conversation where we investigated reconnects, rename it clearly, and back it up.', 'Session discovery and organization.'),
('serving-llms-vllm', 'Deploy this model with vLLM behind an OpenAI-compatible endpoint and tune batching for concurrent requests.', 'Specific high-throughput serving stack.'),
('sdlc-review', 'Review the Kanban handoff from implementation to QA, check its proof, and route the card to the appropriate stage.', 'Verified workflow handoff review.'),
('rust-workspace-disk-hygiene', 'rustc crashed and the linker reports signal 7 in our Rust workspace. Check whether storage exhaustion is the cause first.', 'Specific disk-exhaustion diagnostic signature.'),
('research-paper-writing', 'Help turn these ML experiments into an ICLR submission with a coherent methods section and ablation narrative.', 'ML conference paper development.'),
('requesting-code-review', 'Run the security and quality review of my local changes before I commit them, and fix straightforward findings.', 'Local pre-commit quality gate.'),
('python-debugpy', 'Attach debugpy to the Python worker in the container and inspect the stack at the breakpoint.', 'Python debugger attachment.'),
('provider-routed-surface-decoupling', 'The reasoning menu imports the GLM adapter directly. Move this behavior behind provider metadata so new adapters work automatically.', 'Provider-neutral routing of a harness surface.'),
('product-price-monitor', 'Track this camera listing and notify me when its price drops below 600 dollars.', 'Ongoing product-price alert.'),
('pretext', 'Build a browser typography demo using DOM-free text layout so thousands of text fragments can move smoothly.', 'Specific DOM-free creative layout capability.'),
('powerpoint', 'Update the attached .pptx with a financial results slide and preserve the existing slide master.', 'PowerPoint artifact editing.'),
('popular-web-designs', 'Build a small HTML pricing page inspired by Linear’s visual system, with its restrained spacing and typography.', 'Named existing design-system styling.'),
('plan', 'Write the migration implementation plan as a markdown file under .agent-vesper/plans; stop before executing any steps.', 'Explicit persisted plan-only artifact.'),
('pdf', 'Combine these three PDF invoices into one file and protect the result with the supplied password.', 'PDF merge and security operations.'),
('p5js', 'Make an interactive p5.js sketch where particles swirl toward the pointer on a canvas.', 'Named generative-art framework.'),
('openhue', 'Set the Philips Hue lamps in the study to a warm evening scene using the local controller.', 'Hue device control.'),
('opencode', 'Delegate this feature branch implementation to OpenCode CLI and collect its patch for review.', 'Explicit named coding delegate.'),
('ocr-and-documents', 'This scanned PDF has no selectable text. Extract the page text and preserve page references.', 'OCR and scan extraction, not document editing.'),
('obsidian', 'Search my Obsidian vault for notes about backup rotation and add a linked summary note.', 'Named local note-vault operations.'),
('notion', 'Add these project milestones to the Notion database and update the associated project page.', 'Named remote page/database operations.'),
('node-inspect-debugger', 'Attach to the Node process on its inspect port and evaluate the request object while execution is paused.', 'Node inspector debugging.'),
('nano-pdf', 'In the existing PDF, replace the outdated contact line with this new phone number using natural-language edits.', 'Targeted text replacement in an existing PDF.'),
('merge-reconciler', 'Two agents changed the same interface incompatibly. Act as a neutral reviewer and reconcile the conflicting patches.', 'Third-party reconciliation of agent changes.'),
('meeting-action-items', 'From these pasted meeting notes, extract the decisions and assign each action to its stated owner with a supporting quote.', 'Action extraction from already supplied meeting material.'),
('maps', 'Geocode these three addresses and estimate the driving route between them with OpenStreetMap data.', 'Geocoding and route computation.'),
('manim-video', 'Animate why the derivative of sine is cosine as a narrated Manim scene for a math lesson.', 'Programmatic mathematical animation.'),
('llm-wiki', 'Build an interlinked markdown knowledge base from this reading folder, including topic pages and source backlinks.', 'Interlinked source-grounded markdown knowledge base.'),
('llama-cpp', 'Run this GGUF model locally with llama.cpp and choose CPU settings that fit my machine.', 'Local GGUF inference stack.'),
('interactive-developer', 'Use VesperLens to interview me about the dashboard, build a draft, and keep revising until I explicitly approve the review.', 'Explicit human review state machine.'),
('inspect-live-dom', 'Connect to our running Electron window over CDP and inspect the computed styles of the clipped sidebar.', 'Read live DOM and rendered CSS.'),
('imessage', 'Send this approved text to Sam from Messages on my Mac using the imsg command.', 'Named macOS message transport.'),
('humanizer', 'Rewrite this stiff draft so it sounds like a real colleague wrote it; remove the generic AI phrasing.', 'Voice and AI-ism revision.'),
('huggingface-hub', 'Use the hf command to download the dataset snapshot from this Hugging Face repository.', 'Hub artifact discovery and transfer.'),
('himalaya', 'Use Himalaya to fetch the latest email in my IMAP account and show its headers.', 'Named email CLI retrieval.'),
('harness-skill-gate-mechanics', 'learn_skill refuses with “verify the outcome first” even though I ran tests in a pipeline. Diagnose what the harness needs.', 'Specific skill-learning verification gate.'),
('grounded-citations', 'Audit this report’s factual claims against verifiable sources and attach citations that actually support each claim.', 'Source verification and citation grounding.'),
('google-workspace', 'Help configure the downloaded Google Cloud OAuth client credentials for our Workspace connection.', 'Exact credential setup descriptor, not inferred Gmail automation.'),
('github-repo-management', 'Fork this GitHub repository into my organization and configure the local origin and upstream remotes.', 'Repository creation/fork and remote management.'),
('github-pr-workflow', 'Take my completed branch through opening a GitHub pull request, tracking its CI, and merging after the checks pass.', 'Existing implementation through PR lifecycle.'),
('github-issues', 'Triage the new GitHub issues, label duplicates, and assign the confirmed bugs to their owners.', 'Issue metadata management.'),
('github-issue-to-pr', 'Implement GitHub issue 412 end to end and open a verified pull request that resolves it, reporting the real CI state.', 'Issue-to-implementation-to-PR workflow.'),
('github-code-review', 'Review pull request 91 on GitHub and leave inline comments on the risky diff lines.', 'Review of an existing remote PR.'),
('github-auth', 'Set up SSH authentication for GitHub and verify that gh is logged into the intended account.', 'GitHub identity setup.'),
('gif-search', 'Find a small celebratory GIF on Tenor and download it for the release announcement.', 'GIF search and retrieval.'),
('findmy', 'Use FindMy on my Mac to locate the AirTag attached to my keys.', 'Apple tracking integration.'),
('excalidraw', 'Create an editable Excalidraw scene showing the request flow from browser to gateway to database.', 'Explicit editable diagram format.'),
('evaluating-llms-harness', 'Benchmark this local language model on GSM8K using lm-eval-harness and record the exact configuration.', 'Named model benchmark harness.'),
('end-to-end-workflow-acceptance', 'Before claiming the new interactive feature is finished, prove that a user can complete the whole workflow in the installed release.', 'User-facing installed-artifact acceptance.'),
('email-inbox-triage', 'Sort my unread email by urgency and draft replies for the messages that need a response; leave drafts unsent.', 'Inbox prioritization and safe response drafting.'),
('dogfood', 'Explore the staging checkout flow as a user, look for failures, and capture reproducible evidence for each bug.', 'Exploratory application QA.'),
('docx', 'Revise this Word contract’s .docx headings and tables without losing the template styles.', 'Word document artifact editing.'),
('document-to-action-items', 'Read the attached lease and list every tenant obligation and deadline with the clause that supports it.', 'Document-derived obligations with evidence.'),
('design-md', 'Validate the color and spacing tokens in our DESIGN.md against Google’s design specification and export the corrected file.', 'Named token specification validation.'),
('cross-platform-test-pitfalls', 'This test passes on Linux but fails on Windows in Vesper’s five-target CI. Use the actual job log to identify the portability mistake.', 'Repository cross-platform test diagnosis.'),
('computer-use', 'Use the background desktop controller to move through this native app’s settings without taking focus from my editor.', 'Background desktop interaction.'),
('competitor-news-monitor', 'Watch these three competing vendors and send a cited digest when a material announcement appears.', 'Named-company news monitoring.'),
('comfyui', 'Create a ComfyUI diffusion workflow that generates product images from these prompts.', 'Named diffusion workflow environment.'),
('cold-start-import-deferral', 'The legacy Python glm_acp --version path became slow after a new module-level import. Move heavy imports out of the cold path.', 'Specific legacy CLI import-latency repair.'),
('codex', 'Hand this coding task to the OpenAI Codex command-line agent and collect the resulting changes.', 'Explicit named coding delegate.'),
('codebase-inspection', 'Use pygount to inventory the repository’s languages and report source line counts and ratios.', 'Quantitative repository inventory.'),
('claude-design', 'Create a polished one-off HTML presentation for our launch narrative that we can open directly in a browser.', 'Standalone designed HTML artifact.'),
('claude-code', 'Run Claude Code CLI on this isolated worktree to implement the requested endpoint.', 'Explicit named coding delegate.'),
('box', 'Search our Box folder for the signed contract and update its metadata with the renewal date.', 'Named cloud document service.'),
('blogwatcher', 'Subscribe to these RSS feeds with blogwatcher and report newly published posts.', 'Feed monitoring with named tool.'),
('blocked-page-recovery', 'The article URL returns a WAF challenge. Try supported fallback retrieval methods to obtain the page content.', 'Recovery from blocked retrieval.'),
('baoyu-infographic', 'Turn this set of survey statistics into a styled infographic that shows the main comparisons clearly.', 'Statistical infographic deliverable.'),
('ascii-video', 'Convert this short clip into a colored ASCII animation and export an MP4.', 'Moving ASCII media artifact.'),
('ascii-art', 'Make a static monospace cat illustration I can paste into a terminal welcome banner.', 'Static ASCII art.'),
('arxiv', 'Search arXiv for recent papers by this author about graph diffusion and collect their identifiers.', 'Named research repository search.'),
('architecture-diagram', 'Deliver a dark-themed SVG infrastructure diagram embedded in HTML, showing our VPC, load balancer, and services.', 'Specific SVG infrastructure rendering format.'),
('apple-reminders', 'Add a reminder to my Apple Reminders list for renewing the passport on Monday.', 'Apple task-list operation.'),
('apple-notes', 'Find the packing list in Apple Notes and append the camping equipment using memo.', 'Apple note service operation.'),
('airtable', 'Upsert these customer rows into Airtable using their external IDs so existing records are updated.', 'Named structured cloud data CRUD.'),
('agent-vesper', 'Help me configure Agent Vesper’s native memory and reasoning controls for this project.', 'Use and configuration of Vesper.'),
('add-glm-model-release', 'The legacy Python glm-acp needs the newly shipped GLM model in its registry-driven picker and the full release workflow.', 'Explicit legacy model addition and release.'),
('acp-stdio-smoke-test', 'Smoke-test the legacy Python glm-acp launch over stdio with real input and output pipes after the CLI change.', 'Specific legacy stdio launch validation.'),
]
for skill, prompt, why in positive:
    add('positive', prompt, [skill], why)

no_skill = [
'Thanks, that answers my question.',
'Good morning! How are you?',
'Pause here until I get back.',
'Please keep the next answer under two sentences.',
'What is 17 multiplied by 6?',
'Convert 90 minutes into hours.',
'Spell the word accommodation.',
'What is the plural of mouse?',
'Translate “good evening” into Spanish.',
'Explain the difference between a noun and a verb in one sentence.',
'What does the acronym CPU stand for?',
'Why does ice float on water? A short general explanation is enough.',
'Tell me a harmless joke about a penguin.',
'Give me a rhyming word for moon.',
'Name three colors that appear in a rainbow.',
'What comes after Thursday?',
'How many sides does a hexagon have?',
'Which is larger, three quarters or two thirds?',
'Change “the cats is asleep” to correct grammar.',
'Alphabetize these words: pear, apple, banana.',
'Summarize this sentence in five words: The team postponed its picnic because heavy rain was forecast.',
'Repeat the exact phrase: quiet purple mountain.',
'I prefer tea to coffee.',
'I am taking a break now.',
'Your previous answer was too long.',
'Use British spelling from now on in this conversation.',
'Do not make any changes yet.',
'I changed my mind; cancel that request.',
'What does “measure twice, cut once” mean?',
'Give me a friendly birthday greeting for my cousin.',
'Is 29 a prime number?',
'List the first four positive even numbers.',
'Convert 2.5 kilometres to metres.',
'What is the opposite of transparent?',
'Explain recursion with a simple everyday analogy, without writing code.',
'What is a spreadsheet? Just define the term.',
'What does PDF stand for? Only expand the abbreviation.',
'What is the difference between a meeting agenda and meeting minutes?',
'What does a pull request mean in software development? No repository actions.',
'What is a debugger? Give a general definition.',
'Tell me whether the word “running” has seven letters.',
'Choose between the names Cedar and Birch for a fictional cat.',
'How many centimetres are in a metre?',
'Please answer only yes: did you receive this message?',
'What is the past tense of write?',
'Explain why a shadow changes length during the day.',
'What does the musical term chorus mean? I am asking for a definition.',
'What is a hashtag? Explain the punctuation convention only.',
'I saw a funny GIF today.',
'My colleague likes drawing diagrams on paper.',
'I own a Philips Hue bulb. That is just background information; there is no task.',
'The word “notion” in this sentence means an idea. Is that correct?',
'Can you say “welcome back” in French?',
'What is the remainder when 20 is divided by 6?',
'Describe the taste difference between sweet and sour in simple terms.',
'What is the next letter after Q in the English alphabet?',
'Give me two antonyms for the word loud.',
'The transcript is already handled. Please just acknowledge receipt.',
'No need to research anything; just say good night.',
'I am ready for lunch, and this is not a request to schedule anything.',
]
for prompt in no_skill:
    add('no_skill', prompt, rationale='Conversation, control, or self-contained general answer; no specialized catalog workflow is requested.')

sibling = [
('I pasted the meeting notes below. Extract owners and follow-ups; do not connect to Teams or retrieve any recordings.', ['meeting-action-items'], ['teams-meeting-pipeline'], 'Source material is supplied; source-specific acquisition is unnecessary.'),
('Retrieve the Teams transcript from the meeting identifier first; there are no notes pasted here.', ['teams-meeting-pipeline'], ['meeting-action-items'], 'The requested deliverable is source retrieval, not extraction from supplied notes.'),
('Make a list of duties and due dates from this insurance policy, citing its clauses. This is not meeting material.', ['document-to-action-items'], ['meeting-action-items'], 'Obligations in a document differ from meeting decisions.'),
('Draw the topology as an editable .excalidraw JSON file; the team needs to move the shapes later.', ['excalidraw'], ['architecture-diagram'], 'Requested editable format distinguishes diagram siblings.'),
('Export the infrastructure picture as a dark SVG in a standalone HTML page; do not produce an Excalidraw scene.', ['architecture-diagram'], ['excalidraw'], 'Requested rendering format is explicit.'),
('Create the pitch as a real .pptx file with editable slides, not a browser presentation.', ['powerpoint'], ['claude-design'], 'Artifact format disambiguates presentation tools.'),
('Create a single HTML launch presentation for the browser; no PowerPoint file is wanted.', ['claude-design'], ['powerpoint'], 'Artifact format disambiguates presentation tools.'),
('Extract text from photographed pages in the PDF. Do not change any words or edit the original file.', ['ocr-and-documents'], ['nano-pdf'], 'OCR extraction is not PDF text replacement.'),
('Merge the existing PDFs and add a password; all their text is already readable.', ['pdf'], ['ocr-and-documents','nano-pdf'], 'Structural PDF operation, not OCR or text editing.'),
('Edit the misspelled heading directly in this existing PDF using a natural-language replacement instruction.', ['nano-pdf'], ['ocr-and-documents'], 'PDF editing, not extraction.'),
('Inspect this already-open GitHub pull request and post inline review comments. Do not implement the issue.', ['github-code-review'], ['github-issue-to-pr'], 'Existing PR review is different from issue implementation.'),
('Label and assign GitHub issue 22; do not start a branch or implement it.', ['github-issues'], ['github-issue-to-pr','github-pr-workflow'], 'Issue triage only.'),
('Create a GitHub repository and set its remotes. There is no pull request to open.', ['github-repo-management'], ['github-pr-workflow'], 'Repository setup only.'),
('Fix my GitHub SSH login. Do not create repositories or open pull requests.', ['github-auth'], ['github-repo-management','github-pr-workflow'], 'Authentication prerequisite rather than content operations.'),
('Attach to the Python process with debugpy; the JavaScript frontend is not involved.', ['python-debugpy'], ['node-inspect-debugger'], 'Language and debugger are explicit.'),
('Pause the Node.js worker through its --inspect port; there is no Python process.', ['node-inspect-debugger'], ['python-debugpy'], 'Language and debugger are explicit.'),
('Run the downloaded GGUF locally with llama.cpp; I do not need a multi-user serving endpoint.', ['llama-cpp'], ['serving-llms-vllm'], 'Local runtime choice is explicit.'),
('Set up vLLM batching for our shared inference endpoint; do not switch us to llama.cpp.', ['serving-llms-vllm'], ['llama-cpp'], 'Serving runtime choice is explicit.'),
('Download the model snapshot with hf; do not launch it or run benchmarks.', ['huggingface-hub'], ['llama-cpp','evaluating-llms-harness'], 'Artifact acquisition only.'),
('Benchmark the model with lm-eval-harness on MMLU; do not publish an ML paper or configure experiment tracking.', ['evaluating-llms-harness'], ['research-paper-writing','weights-and-biases'], 'Benchmark execution rather than adjacent ML workflows.'),
('Record the experiment sweep in W&B. The benchmark scores already exist; do not rerun evaluation.', ['weights-and-biases'], ['evaluating-llms-harness'], 'Tracking existing results rather than generating scores.'),
('Add this task to Apple Reminders, not to my notes app.', ['apple-reminders'], ['apple-notes','obsidian'], 'Task store is explicit.'),
('Append this paragraph to the note in Apple Notes using memo, not to Obsidian.', ['apple-notes'], ['obsidian','apple-reminders'], 'Note store is explicit.'),
('Update a page inside my Obsidian vault, not our Notion workspace.', ['obsidian'], ['notion','apple-notes'], 'Knowledge-store identity is explicit.'),
('Query records in Airtable, not a Notion database or an Excel workbook.', ['airtable'], ['notion','xlsx'], 'Structured-store identity is explicit.'),
('Make a static ASCII owl to paste into a text file. No animation or video export.', ['ascii-art'], ['ascii-video'], 'Static text deliverable.'),
('Render the video as moving ASCII and save an MP4; a single text image is insufficient.', ['ascii-video'], ['ascii-art'], 'Temporal media deliverable.'),
('Produce a mel spectrogram from this audio file. Do not compose music or write lyrics.', ['songsee'], ['songwriting-and-ai-music'], 'Audio analysis rather than composition.'),
('Write lyrics and a Suno style prompt; I do not have an audio recording to analyse.', ['songwriting-and-ai-music'], ['songsee'], 'Composition rather than audio analysis.'),
('Inspect computed CSS in the live Electron app through CDP, without clicking around the desktop.', ['inspect-live-dom'], ['computer-use'], 'Read-only browser inspection, not desktop control.'),
]
for prompt, good, bad, why in sibling:
    add('sibling', prompt, good, why, forbidden=bad)

# Compatibility cases deliberately contain execution-only constraints. They do
# not imply a general inability to explain, plan, or discuss the relevant tool.
resource = [
('Use the OpenHue controller to change my lights now. That controller is unavailable here; no network access or fallback tool is permitted. If execution is impossible, report it without loading an execution workflow.', 'openhue', ['openhue'], ['network']),
('Send the approved iMessage now, but this Linux session has no imsg tool or macOS host. No remote access or alternate messaging service is allowed. Report the missing capability instead of starting the workflow.', 'imessage', ['imsg','macos-host'], ['remote_access']),
('Locate my AirTag now. This environment has no FindMy.app or Mac access, and using any remote account is forbidden. Only report whether this can run here.', 'findmy', ['FindMy.app','macos-host'], ['remote_access']),
('Change the TouchDesigner project through twozero now. There is no twozero MCP connection, and installing or connecting one is not permitted. If unavailable, stop with that fact.', 'touchdesigner-mcp', ['twozero'], ['install_tools','connect_services']),
('Delegate this code change to OpenCode CLI now. OpenCode is missing, installation is forbidden, and no substitute delegate may run. Report that limitation without starting a delegate workflow.', 'opencode', ['opencode'], ['install_tools','substitute_delegate']),
('Run Claude Code CLI on the branch now. It is not installed, installation is denied, and other coding delegates are not allowed. Return an unavailable-capability result.', 'claude-code', ['claude'], ['install_tools','substitute_delegate']),
('Have Codex CLI execute the task now. The CLI is absent and neither installation nor another delegation service is permitted. Only report the execution blocker.', 'codex', ['codex'], ['install_tools','substitute_delegate']),
('Update the Airtable record now. All network access is denied for this task, no offline queue is allowed, and I only want a truthful blocked result if the write cannot execute.', 'airtable', [], ['network','queue_external_writes']),
('Post this message to X now. External writes are forbidden in this session and I am not requesting a draft. If posting is prohibited, report that without activating posting tools.', 'xurl', [], ['external_writes']),
('Edit the Notion page now. This session permits no remote page writes or later queued edits. If the action is denied, state the constraint rather than beginning a Notion workflow.', 'notion', [], ['external_writes','queue_external_writes']),
('Create the Box share now. This task has no permission to create external shares and no approval may be inferred from skill selection. Return the blocked result.', 'box', [], ['create_external_shares']),
('Control the desktop to change that setting now. Desktop-control permission is denied and neither simulated clicks nor alternative remote-control tools are allowed. Only report the denial.', 'computer-use', [], ['desktop_control']),
]
for prompt, skill, missing, denied in resource:
    add('resource', prompt, rationale='The execution-only request explicitly requires abstention when its mandatory capability or authorization is absent.',
        forbidden=[skill], unavailable=missing, denied=denied)

ambiguous = [
('Make this presentation look better.', ['powerpoint','claude-design'], 'No file type or rendering medium is supplied.'),
('Put this in my notes.', ['obsidian','apple-notes','notion'], 'The destination note store is unspecified.'),
('Help me with this PDF.', ['pdf','ocr-and-documents','nano-pdf'], 'The operation is unspecified.'),
('I need a diagram of how these components connect.', ['architecture-diagram','excalidraw'], 'The editable/rendered diagram medium is unspecified.'),
('Have another coding agent take care of this.', ['codex','claude-code','opencode'], 'No delegate or installed executor is specified.'),
('Help me turn this song idea into something concrete.', ['songwriting-and-ai-music'], 'Composition help is plausible but the desired artifact is underspecified.'),
('Can we organise the work for the next stage?', ['plan','weekly-review-planning'], 'Could refer to a software plan or personal commitment planning.'),
('Check why the program is stuck.', ['systematic-debugging'], 'Root-cause workflow is plausible; no language or reproducible symptom is supplied.'),
]
for prompt, good, why in ambiguous:
    add('ambiguous', prompt, good, why, clarity='ambiguous', abstain=True)

assert len(positive) >= 80 and len(no_skill) >= 60 and len(sibling)+len(resource) >= 40
assert len({c['prompt'] for c in cases}) == len(cases)
for c in cases:
    assert len(c['expected_skill_ids']) <= 3
    assert set(c['expected_skill_ids'] + c['forbidden_skill_ids']) <= known
    assert not set(c['expected_skill_ids']) & set(c['forbidden_skill_ids'])
    assert c['acceptable_abstention'] == (c['clarity'] == 'ambiguous')
corpus = dict(
    schema_version=1,
    status='frozen_before_predictions',
    provenance=dict(catalog_sha256=hashlib.sha256(catalog_bytes).hexdigest(),
                    catalog_entries=len(catalog),
                    author_role='independent_evaluator',
                    previous_corpus_read=False, predictions_read=False,
                    router_implementation_read=False, skill_bodies_read=False),
    scoring_contract=dict(
        expected_skill_ids='Set of acceptable task-relevant skill IDs; not an instruction to select every listed alternative.',
        positive='Require a nonempty selection drawn only from expected_skill_ids; max 3 selections.',
        no_skill='Require empty selection.',
        sibling='Require a nonempty selection drawn only from expected_skill_ids; explicitly forbidden siblings must not activate.',
        resource='Require empty selection under the supplied unavailable_tools/denied_permissions; score separately from unrestricted semantic routing.',
        ambiguous='Empty selection is acceptable; otherwise require all selections to be in expected_skill_ids. Report separately from clear-task accuracy.',
        forbidden='A selected forbidden_skill_id is always a failure.',
        scope='Only task-directed routing is scored. Always-on system instructions are not inferred as additional selectable skills.'),
    counts=dict(collections.Counter(c['kind'] for c in cases)), cases=cases)
payload = (json.dumps(corpus, indent=2, ensure_ascii=False)+'\n').encode()
path = HERE/'skill-routing-independent-corpus.json'
path.write_bytes(payload)
sha = hashlib.sha256(payload).hexdigest()
(HERE/'skill-routing-independent-corpus.sha256').write_text(f'{sha}  skill-routing-independent-corpus.json\n')
print(json.dumps(dict(sha256=sha, cases=len(cases), counts=corpus['counts'],
                     distinct_positive_skills=len({s for c in cases if c['kind']=='positive' for s in c['expected_skill_ids']}))))

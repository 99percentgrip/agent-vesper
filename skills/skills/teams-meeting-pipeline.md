---
name: teams-meeting-pipeline
description: Microsoft Teams meeting records and transcripts via the Graph REST API (curl), with summary and action-item extraction.
version: 2.0.0
author: Agent Vesper library (Graph-native rewrite)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [teams, meetings, transcripts, graph, summaries]
prerequisites:
  env_vars: [MSGRAPH_TOKEN]
  commands: [curl, jq]
---

# Teams Meeting Pipeline (Graph API)

Retrieve Microsoft Teams meeting records and transcripts through the
Microsoft Graph REST API and turn them into summaries and action items.

## Prerequisites (fail truthfully if absent)

- `MSGRAPH_TOKEN` — a valid access token for Microsoft Graph, obtained by
  the user through their own Azure AD app or `az account get-access-token`.
  Required scopes: `OnlineMeetings.Read`, `OnlineMeetingTranscript.Read`,
  `Calendars.Read`.
- `curl` and `jq` on PATH.

Never invent or simulate meeting data. If the token is missing or a call
returns 401/403, report that and stop.

## Procedure

1. Verify the token works before anything else:

       curl -s -H "Authorization: Bearer $MSGRAPH_TOKEN" \
         "https://graph.microsoft.com/v1.0/me" | jq -r '.userPrincipalName'

2. List recent online meetings (calendar-driven view):

       curl -s -H "Authorization: Bearer $MSGRAPH_TOKEN" \
         "https://graph.microsoft.com/v1.0/me/events?\$select=subject,onlineMeeting,start,end&\$top=25&\$orderby=start/desc" \
         | jq '.value[] | {subject, start: .start.dateTime, joinUrl: .onlineMeeting.joinUrl}'

3. For a target meeting, resolve the online meeting artifact:

       curl -s -H "Authorization: Bearer $MSGRAPH_TOKEN" \
         "https://graph.microsoft.com/v1.0/users/<userId>/onlineMeetings?\$filter=JoinWebUrl%20eq%20'<joinUrl>'"

4. Fetch the transcript list, then a transcript (content is
   text/vtt when available):

       curl -s -H "Authorization: Bearer $MSGRAPH_TOKEN" \
         ".../onlineMeetings/<meetingId>/transcripts" | jq '.value[] | {id, createdDateTime}'
       curl -s -H "Authorization: Bearer $MSGRAPH_TOKEN" \
         ".../onlineMeetings/<meetingId>/transcripts/<id>/content" -o transcript.vtt

   Some tenants require `?$metadata=model%3Dpublic` for cross-tenant
   transcript access; if content returns 403, state that access to
   transcript content is blocked by policy rather than guessing.

5. Parse the VTT (speaker turns + timestamps), then produce:
   - a 5-10 bullet summary with per-claim timestamps,
   - decisions made,
   - action items as `owner — action — due` rows,
   - open questions.

6. Record output files under the project workspace (never under the user's
   home), and cite timestamps for every extracted obligation.

## Failure modes

- 401/403: token absent, expired, or scope missing — say so, do not retry
  with fabricated data.
- Empty transcript list: transcription may be disabled by policy for the
  tenant or the meeting; report the empty result and the likely cause.
- VTT not available: some meetings only retain `meetingIntelligence`
  metadata; report what was actually returned.

## Verification

- Step 1 prints a real `userPrincipalName`.
- Every summary bullet carries a timestamp resolvable in `transcript.vtt`.

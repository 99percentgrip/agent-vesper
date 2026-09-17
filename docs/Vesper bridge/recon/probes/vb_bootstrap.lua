-- Vesper Bridge bootstrap probe v3: sandboxed-IO-safe.
-- Reads nothing from disk; returns results via PRINT ONLY (Console captures output).
-- The host observes the Console window? No — instead we publish via the project itself:
-- create/refresh a timeline marker with the encoded handshake (API-visible, no io).
local out = {}
local ok, err = pcall(function()
  out.version = tostring(resolve:GetVersionString())
  out.product = tostring(resolve:GetProductName())
  local pm = resolve:GetProjectManager()
  local proj = pm:GetCurrentProject()
  out.project = proj and tostring(proj:GetName()) or "none"
  out.timeline_count = proj and tostring(proj:GetTimelineCount()) or "0"
  out.media_pool = proj and tostring(proj:GetMediaPool() ~= nil) or "?"
end)
if not ok then out.err = tostring(err) end

local payload = table.concat({
  "VB3",
  out.version or "?",
  out.product or "?",
  out.project or "?",
  out.timeline_count or "?",
  out.media_pool or "?",
  out.err or "-",
}, "|")

-- Publish path 1: print (visible to human; also lands in Resolve's console log)
print(payload)

-- Publish path 2: durable, host-readable via the Project's comment field (no io needed)
local pm2 = resolve:GetProjectManager()
local proj2 = pm2 and pm2:GetCurrentProject()
if proj2 then
  local ok2, err2 = pcall(function()
    proj2:SetComments(payload)
  end)
  if not ok2 then print("SETCOMMENTS_ERR " .. tostring(err2)) end
end

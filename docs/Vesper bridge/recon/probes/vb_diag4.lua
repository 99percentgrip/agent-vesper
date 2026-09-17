-- Vesper Bridge diag4: run the EXACT render_draft sequence step by step,
-- reporting each return value, to find which call in sequence fails.
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 200 then s = s:sub(1, 200) .. "~" end
  return s
end

local OUTDIR = "/home/Alex/Videos/vb-fixture/out"

local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
if not p then print("VBERR|diag4|no project") return end

-- step 1: format+codec
local ok1 = p:SetCurrentRenderFormatAndCodec("MKV", "ProRes422HQ")
print("VBOK|diag4|s1_fmtcodec=" .. tostring(ok1))

-- step 2: settings WITHOUT CustomName first
local rs = { TargetDir = OUTDIR, SelectAllFrames = true }
local ok2 = p:SetRenderSettings(rs)
print("VBOK|diag4|s2_settings_noCustomName=" .. tostring(ok2))

-- step 3: AddRenderJob
local ok3, jobId = pcall(function() return p:AddRenderJob() end)
print("VBOK|diag4|s3_addjob=" .. tostring(ok3) .. "|job=" .. esc(jobId))

if ok3 and jobId and jobId ~= "" then
  -- step 4: status
  local st = p:GetRenderJobStatus(jobId)
  print("VBOK|diag4|s4_status=" .. esc(st and st["JobStatus"] or "?"))
end

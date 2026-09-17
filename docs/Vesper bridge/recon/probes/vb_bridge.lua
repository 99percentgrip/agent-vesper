-- Vesper Bridge — fixed command registry for DaVinci Resolve free 21.1 (Console route)
-- v5: status/import/timeline/render/verify. TYPED COMMANDS ONLY. No eval of model text.
--
-- Adversarial content policy (PRD §8): every string that originates from media
-- metadata, clip names, project names, or file paths is UNTRUSTED. It is
-- escaped (VBCSV: |, newline, CR, quotes stripped) before it reaches any
-- output channel, and it is never executed, never concat'd into a command.
-- Output channels: print (human) + marker customData (host reads via GetMarkerCustomData).

local MAXLEN = 1200

local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 200 then s = s:sub(1, 200) .. "~" end
  return s
end

local function csv(parts)
  return table.concat(parts, "|")
end

local function say(line)
  print(line)
  -- also publish to marker channel at frame 0 of current timeline (best effort)
  local ok, err = pcall(function()
    local pm = resolve:GetProjectManager()
    local p = pm and pm:GetCurrentProject()
    if not p then return end
    local t = p:GetCurrentTimeline()
    if not t then return end
    local prev = t:GetMarkerCustomData(0) or ""
    local combined = (prev .. "\n" .. line)
    if #combined > MAXLEN then combined = combined:sub(#combined - MAXLEN + 1) end
    t:UpdateMarkerCustomData(0, combined)
  end)
  if not ok then
    -- marker channel unavailable; print channel already fired
    pcall(function() print("VBWARN|marker_channel_unavailable|" .. esc(tostring(err))) end)
  end
end

local function fail(op, msg)
  say(csv({ "VBERR", op, esc(msg) }))
end

-- ---------- op: status ----------
local function op_status()
  local pm = resolve:GetProjectManager()
  if not pm then return fail("status", "no project manager") end
  local p = pm:GetCurrentProject()
  local name = p and p:GetName() or "none"
  local tl = p and p:GetCurrentTimeline()
  local tlname = tl and tl:GetName() or "none"
  say(csv({ "VBOK", "status",
    "app=" .. esc(resolve:GetProductName()),
    "ver=" .. esc(resolve:GetVersionString()),
    "proj=" .. esc(name),
    "tl=" .. esc(tlname),
    "tlcount=" .. tostring(p and p:GetTimelineCount() or 0) }))
end

-- ---------- op: fixture_import (FIXED path, not model-controlled) ----------
local FIXTURE = "/home/Alex/Videos/vb-fixture/fixture_master.webm"

local function op_fixture_import()
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return fail("fixture_import", "no current project") end
  local mp = p:GetMediaPool()
  if not mp then return fail("fixture_import", "no media pool") end
  local root = mp:GetRootFolder()
  if not root then return fail("fixture_import", "no root folder") end
  mp:SetCurrentFolder(root)
  -- Evidence (2s diag): legacy string-list form returns 1 item on free 21.1;
  -- the ImportClipInfo dict form returns nil on this build. Use the proven form.
  local items = mp:ImportMedia({ FIXTURE })
  if not items or #items == 0 then
    return fail("fixture_import", "ImportMedia returned empty (path unreadable or unsupported)")
  end
  local it = items[1]
  local clipname = "?"
  pcall(function() clipname = it:GetName() end)
  say(csv({ "VBOK", "fixture_import",
    "clip=" .. esc(clipname),
    "path=" .. esc(FIXTURE),
    "count=" .. tostring(#items) }))
end

-- ---------- op: timeline_from_fixture ----------
local function op_timeline_from_fixture()
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return fail("timeline_from_fixture", "no project") end
  local mp = p:GetMediaPool()
  local root = mp:GetRootFolder()
  local clips = root:GetClipList()
  if not clips or #clips == 0 then
    return fail("timeline_from_fixture", "no clips in root — run fixture_import first")
  end
  local target = nil
  for _, c in ipairs(clips) do
    local n = "?"
    pcall(function() n = c:GetName() end)
    if n and n:find("fixture_master", 1, true) then target = c break end
  end
  if not target then
    return fail("timeline_from_fixture", "fixture_master clip not found in root")
  end
  local mp2 = p:GetMediaPool()
  local tl = mp2:CreateTimelineFromClips("VB Slice Timeline", { { mediaPoolItem = target } })
  if not tl then
    return fail("timeline_from_fixture", "CreateTimelineFromClips returned nil")
  end
  p:SetCurrentTimeline(tl)
  local dur = "?"
  pcall(function() dur = tostring(tl:GetEndFrame() - tl:GetStartFrame()) end)
  say(csv({ "VBOK", "timeline_from_fixture",
    "tl=" .. esc(tl:GetName()),
    "dur_frames=" .. esc(dur) }))
end

-- ---------- op: render_draft (FIXED dir; format from MEASURED free-edition codecs) ----------
-- Evidence (diag2, free 21.1.0.0017_lite): MP4 = {APVYUV422_10 only}; NO H.264/H.265
-- encoders in free (Studio-only). MKV/QuickTime offer ProRes/DNxHD/FFV1.
-- Chosen: MKV + Apple ProRes 422 (broadly decodable, professional intermediate).
local OUTDIR = "/home/Alex/Videos/vb-fixture/out"

local function op_render_draft()
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return fail("render_draft", "no project") end
  local fmts = p:GetRenderFormats()
  -- map is name->extension; find the MKV key
  local fmt = nil
  for k, v in pairs(fmts or {}) do
    if tostring(v):lower() == "mkv" then fmt = k break end
  end
  if not fmt then return fail("render_draft", "mkv not in GetRenderFormats") end
  local codecs = p:GetRenderCodecs("mkv") or {}
  local codec = nil
  local codecName = "?"
  for cid, cname in pairs(codecs) do
    if tostring(cname) == "ProRes422HQ" then codec = cid codecName = tostring(cname) break end
  end
  if not codec then
    for cid, cname in pairs(codecs) do
      if tostring(cname) == "ProRes422" then codec = cid codecName = tostring(cname) break end
    end
  end
  if not codec then
    for cid, cname in pairs(codecs) do
      if tostring(cname):find("FFV1") then codec = cid codecName = tostring(cname) break end
    end
  end
  if not codec then return fail("render_draft", "no ProRes/FFV1 codec in mkv") end
  if not p:SetCurrentRenderFormatAndCodec(fmt, codec) then
    return fail("render_draft", "SetCurrentRenderFormatAndCodec failed fmt=" .. esc(fmt) .. " codec=" .. esc(codecName))
  end
  -- Evidence (diag4): CustomName in SetRenderSettings breaks AddRenderJob on free 21.1.
  -- TargetDir + SelectAllFrames alone: works (job created, status Ready).
  local rs = { TargetDir = OUTDIR, SelectAllFrames = true }
  p:SetRenderSettings(rs)
  local jobId = p:AddRenderJob()
  if not jobId or jobId == "" then return fail("render_draft", "AddRenderJob returned empty") end
  say(csv({ "VBOK", "render_draft",
    "job=" .. esc(jobId),
    "dir=" .. esc(OUTDIR),
    "fmt=mkv",
    "codec=" .. esc(codecName) }))
end

-- ---------- op: render_status ----------
local function op_render_status()
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return fail("render_status", "no project") end
  local jobs = p:GetRenderJobList()
  if not jobs or #jobs == 0 then return fail("render_status", "no jobs") end
  local j = jobs[#jobs]
  local id = j["JobId"]
  local st = p:GetRenderJobStatus(id)
  say(csv({ "VBOK", "render_status",
    "job=" .. esc(id),
    "status=" .. esc(st and st["JobStatus"] or "?"),
    "pct=" .. tostring(st and st["CompletionPercentage"] or -1),
    "err=" .. esc(st and st["Error"] or "-") }))
end

-- ---------- op: render_start (starts the queued job) ----------
local function op_render_start()
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return fail("render_start", "no project") end
  local jobs = p:GetRenderJobList()
  if not jobs or #jobs == 0 then return fail("render_start", "no jobs queued") end
  local ids = {}
  for _, j in ipairs(jobs) do ids[#ids + 1] = j["JobId"] end
  local ok = p:StartRendering(ids)
  say(csv({ "VBOK", "render_start", "started=" .. tostring(ok), "jobs=" .. tostring(#ids) }))
end

-- ---------- dispatcher (typed only) ----------
local ops = {
  status = op_status,
  fixture_import = op_fixture_import,
  timeline_from_fixture = op_timeline_from_fixture,
  render_draft = op_render_draft,
  render_start = op_render_start,
  render_status = op_render_status,
}

local function run(op)
  local f = ops[op]
  if not f then
    return fail("dispatch", "unknown op (allowed: " .. table.concat((function()
      local t = {} for k, _ in pairs(ops) do t[#t + 1] = k end table.sort(t) return t
    end)(), ",") .. ")")
  end
  local ok, err = pcall(f)
  if not ok then fail(op, "panic: " .. tostring(err)) end
end

-- Console paste usage:  dofile(...)  then run("status")
run_result = run
_G.vb_run = run
say("VBOK|ready|ops=status,fixture_import,timeline_from_fixture,render_draft,render_start,render_status")

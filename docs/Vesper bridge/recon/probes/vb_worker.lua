-- Vesper Bridge WORKER (free 21.1 Console route) — paste ONCE per Resolve launch.
-- After this single paste, Bridge drives Resolve via command files. No more pastes.
--
-- Protocol (host <-> worker, via /home/Alex/Videos/vb-fixture/ipc/):
--   host writes  cmd.json     {"id":N,"op":"name","args":{...}}
--   worker writes result.json {"id":N,"ok":bool,"data":"..."} then deletes cmd.json
--   worker writes beat (timestamp) every poll — liveness signal
--
-- SECURITY: op must be one of the fixed registry names; args are validated typed
-- values. NO Lua source is ever read or executed from the command file. Media
-- strings in results are escaped. Poll loop is bounded (MAX_SECONDS).

local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 300 then s = s:sub(1, 300) .. "~" end
  return s
end

local IPC = "/home/Alex/Videos/vb-fixture/ipc"
local CMD = IPC .. "/cmd.json"
local RES = IPC .. "/result.json"
local BEAT = IPC .. "/beat"
local MAX_SECONDS = 8 * 3600 -- 8h bound
local POLL = 0.5

local function writef(path, content)
  local ok, err = pcall(function() bmd.writefile(path, content) end)
  return ok, err
end

local function readf(path)
  local ok, content = pcall(function() return bmd.readfile(path) end)
  if ok then return content end
  return nil
end

-- ---------- typed ops (same registry as vb_bridge.lua, file-channel edition) ----------
local FIXTURE = "/home/Alex/Videos/vb-fixture/fixture_master.webm"
local OUTDIR = "/home/Alex/Videos/vb-fixture/out"

local function op_status(args)
  local pm = resolve:GetProjectManager()
  local p = pm and pm:GetCurrentProject()
  if not p then return false, "no project" end
  local tl = p:GetCurrentTimeline()
  return true, "app=" .. esc(resolve:GetProductName())
    .. "|ver=" .. esc(resolve:GetVersionString())
    .. "|proj=" .. esc(p:GetName())
    .. "|tl=" .. esc(tl and tl:GetName() or "none")
    .. "|tlcount=" .. tostring(p:GetTimelineCount())
end

local function op_fixture_import(args)
  local pm = resolve:GetProjectManager()
  local p = pm and pm:GetCurrentProject()
  if not p then return false, "no project" end
  local mp = p:GetMediaPool()
  mp:SetCurrentFolder(mp:GetRootFolder())
  local items = mp:ImportMedia({ FIXTURE }) -- string form (measured working)
  if not items or #items == 0 then return false, "ImportMedia empty" end
  return true, "clip=" .. esc(items[1] and items[1]:GetName() or "?") .. "|count=" .. #items
end

local function op_timeline_from_fixture(args)
  local pm = resolve:GetProjectManager()
  local p = pm and pm:GetCurrentProject()
  local mp = p:GetMediaPool()
  local clips = mp:GetRootFolder():GetClipList()
  for _, c in ipairs(clips or {}) do
    local n = c:GetName()
    if n and n:find("fixture_master", 1, true) then
      local tl = mp:CreateTimelineFromClips("VB Slice Timeline", { c })
      if not tl then return false, "CreateTimelineFromClips nil" end
      p:SetCurrentTimeline(tl)
      return true, "tl=" .. esc(tl:GetName()) .. "|dur=" .. tostring(tl:GetEndFrame() - tl:GetStartFrame())
    end
  end
  return false, "fixture clip not in pool"
end

local function op_render_draft(args)
  local p = resolve:GetProjectManager():GetCurrentProject()
  if not p then return false, "no project" end
  local fmts = p:GetRenderFormats()
  local fmt = nil
  for k, v in pairs(fmts or {}) do
    if tostring(v):lower() == "mkv" then fmt = k break end
  end
  if not fmt then return false, "no mkv format" end
  if not p:SetCurrentRenderFormatAndCodec(fmt, "ProRes422HQ") then
    return false, "SetCurrentRenderFormatAndCodec failed"
  end
  p:SetRenderSettings({ TargetDir = OUTDIR, SelectAllFrames = true })
  local jobId = p:AddRenderJob()
  if not jobId or jobId == "" then return false, "AddRenderJob empty" end
  return true, "job=" .. esc(jobId)
end

local function op_render_start(args)
  local p = resolve:GetProjectManager():GetCurrentProject()
  local jobs = p:GetRenderJobList()
  if not jobs or #jobs == 0 then return false, "no jobs" end
  local ids = {}
  for _, j in ipairs(jobs) do ids[#ids + 1] = j["JobId"] end
  local ok = p:StartRendering(ids)
  return ok, "jobs=" .. #ids
end

local function op_render_status(args)
  local p = resolve:GetProjectManager():GetCurrentProject()
  local jobs = p:GetRenderJobList()
  if not jobs or #jobs == 0 then return false, "no jobs" end
  local j = jobs[#jobs]
  local st = p:GetRenderJobStatus(j["JobId"])
  return true, "job=" .. esc(j["JobId"])
    .. "|status=" .. esc(st and st["JobStatus"] or "?")
    .. "|pct=" .. tostring(st and st["CompletionPercentage"] or -1)
    .. "|err=" .. esc(st and st["Error"] or "-")
end

-- EDIT OPS (typed, bounded)
local function op_set_marker(args)
  -- args: {frame=N, name=S(<=80), note=S(<=200)}
  local p = resolve:GetProjectManager():GetCurrentProject()
  local tl = p:GetCurrentTimeline()
  if not tl then return false, "no timeline" end
  local frame = tonumber(args and args.frame) or 0
  local name = esc(args and args.name or "vb_marker")
  local note = esc(args and args.note or "")
  local ok = tl:AddMarker(math.floor(frame), "Blue", name, note, 1)
  return ok, "frame=" .. math.floor(frame)
end

local function op_get_state(args)
  local p = resolve:GetProjectManager():GetCurrentProject()
  if not p then return false, "no project" end
  local tl = p:GetCurrentTimeline()
  if not tl then return true, "no_timeline" end
  return true, "tl=" .. esc(tl:GetName())
    .. "|start=" .. tostring(tl:GetStartFrame())
    .. "|end=" .. tostring(tl:GetEndFrame())
    .. "|trackcount=" .. tostring(tl:GetTrackCount("video"))
end

local ops = {
  status = op_status,
  fixture_import = op_fixture_import,
  timeline_from_fixture = op_timeline_from_fixture,
  render_draft = op_render_draft,
  render_start = op_render_start,
  render_status = op_render_status,
  set_marker = op_set_marker,
  get_state = op_get_state,
}

-- ---------- worker loop ----------
print("VBWORKER|starting|ipc=" .. IPC .. "|max_hours=8")

-- ensure ipc dir (bmd has no mkdir; try writing beat — if it fails, tell the human)
local beatok = pcall(function() bmd.writefile(BEAT, tostring(os.time())) end)
if not beatok then
  print("VBWORKER|ERR|cannot_write_ipc_dir|create_it:" .. IPC)
  return
end

local t0 = os.time()
local processed = 0
while (os.time() - t0) < MAX_SECONDS do
  -- liveness
  pcall(function() bmd.writefile(BEAT, tostring(os.time())) end)

  local cmd = readf(CMD)
  if cmd and type(cmd) == "table" and cmd.op and ops[cmd.op] then
    local id = cmd.id or -1
    local ok, data
    local ran, err = pcall(function()
      ok, data = ops[cmd.op](cmd.args or {})
    end)
    if not ran then ok, data = false, "panic: " .. esc(err) end
    pcall(function()
      bmd.writefile(RES, { id = id, ok = ok, data = tostring(data) })
    end)
    pcall(function()
      if bmd.deletefile then bmd.deletefile(CMD) end
    end)
    processed = processed + 1
    print("VBWORKER|done|id=" .. tostring(id) .. "|op=" .. esc(cmd.op) .. "|ok=" .. tostring(ok) .. "|n=" .. processed)
  elseif cmd and type(cmd) == "table" and cmd.op then
    -- unknown op: refuse, but ack so host is not stuck
    pcall(function()
      bmd.writefile(RES, { id = cmd.id or -1, ok = false, data = "unknown_op:" .. esc(cmd.op) })
    end)
    pcall(function()
      if bmd.deletefile then bmd.deletefile(CMD) end
    end)
    print("VBWORKER|refused|op=" .. esc(cmd.op))
  end

  bmd.wait(POLL)
end

print("VBWORKER|stopped|reason=max_time|processed=" .. processed)

-- Vesper Bridge WORKER v2 — free 21.1 Console route, FFI file channel.
-- ONE paste per Resolve launch. After that: host drives Resolve entirely via files.
--
-- Channel (verified live 2026-09-16):
--   host -> worker: /home/Alex/Videos/vb-fixture/ipc/cmd    (JSON-ish line)
--   worker -> host: /home/Alex/Videos/vb-fixture/ipc/result (JSON-ish line)
--   liveness:       /home/Alex/Videos/vb-fixture/ipc/beat   (mtime)
--
-- SECURITY:
--   * op names come from a FIXED registry; unknown -> refused + acked.
--   * NO string is ever load()'d or executed. JSON args are parsed by a tiny
--     hand parser that only extracts quoted string / numeric field values.
--   * App-originated strings are escaped before embedding in results.
--   * The loop is time-bounded; every op runs under pcall.

local IPC = "/home/Alex/Videos/vb-fixture/ipc"
local CMDF = IPC .. "/cmd"
local RESF = IPC .. "/result"
local BEAT = IPC .. "/beat"
local MAX_SECONDS = 8 * 3600
local POLL = 0.4

ffi.cdef[[
  typedef struct FILE FILE;
  FILE *fopen(const char *path, const char *mode);
  int fclose(FILE *f);
  size_t fread(void *ptr, size_t sz, size_t n, FILE *f);
  size_t fwrite(const void *ptr, size_t sz, size_t n, FILE *f);
  int remove(const char *path);
  int stat(const char *path, void *buf);
]]

local function wfile(path, content)
  local f = ffi.C.fopen(path, "w")
  if f == nil then return false end
  ffi.C.fwrite(content, 1, #content, f)
  ffi.C.fclose(f)
  return true
end

local function rfile(path, maxlen)
  local f = ffi.C.fopen(path, "r")
  if f == nil then return nil end
  maxlen = maxlen or 65536
  local buf = ffi.new("char[?]", maxlen)
  local n = ffi.C.fread(buf, 1, maxlen, f)
  ffi.C.fclose(f)
  if n <= 0 then return "" end
  return ffi.string(buf, n)
end

local function exists(path)
  local buf = ffi.new("char[256]")
  return ffi.C.stat(path, buf) == 0
end

local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"\\%c]", " ")
  if #s > 300 then s = s:sub(1, 300) .. "~" end
  return s
end

-- tiny arg parser: extracts "key":"string" and "key":number from a flat JSON object
local function parse_args(s)
  local args = {}
  if not s then return args end
  -- two targeted passes: strings, then numbers
  for k, v in s:gmatch([=["(%w-)"%s*:%s*"([^"]*)""]=]) do
    if #k > 0 and #v <= 4096 then args[k] = v end
  end
  for k, v in s:gmatch([=["(%w-)"%s*:%s*(%-?%d+%.?%d*)]=]) do
    if #k > 0 then args[k] = tonumber(v) end
  end
  return args
end

-- ---------- typed ops ----------
local FIXTURE = "/home/Alex/Videos/vb-fixture/fixture_master.webm"
local OUTDIR = "/home/Alex/Videos/vb-fixture/out"

local function cur()
  local p = resolve:GetProjectManager():GetCurrentProject()
  return p
end

local ops = {}
-- Extension hook (2ag): later-loaded files may register additional ops.
_G.vb_ops_table = ops

ops.status = function(args)
  local p = cur()
  if not p then return "no_project" end
  local tl = p:GetCurrentTimeline()
  return "app=" .. esc(resolve:GetProductName())
    .. "|ver=" .. esc(resolve:GetVersionString())
    .. "|proj=" .. esc(p:GetName())
    .. "|tl=" .. esc(tl and tl:GetName() or "none")
    .. "|tlcount=" .. tostring(p:GetTimelineCount())
end

ops.fixture_import = function(args)
  local p = cur()
  local mp = p:GetMediaPool()
  mp:SetCurrentFolder(mp:GetRootFolder())
  local items = mp:ImportMedia({ FIXTURE })
  if not items or #items == 0 then return nil, "import_empty" end
  return "clip=" .. esc(items[1] and items[1]:GetName() or "?") .. "|count=" .. #items
end

ops.timeline_from_fixture = function(args)
  local p = cur()
  local mp = p:GetMediaPool()
  for _, c in ipairs(mp:GetRootFolder():GetClipList() or {}) do
    if c:GetName():find("fixture_master", 1, true) then
      local tl = mp:CreateTimelineFromClips("VB Slice Timeline", { c })
      if not tl then return nil, "create_failed" end
      p:SetCurrentTimeline(tl)
      return "tl=" .. esc(tl:GetName()) .. "|dur=" .. tostring(tl:GetEndFrame() - tl:GetStartFrame())
    end
  end
  return nil, "fixture_not_in_pool"
end

ops.render_draft = function(args)
  local p = cur()
  local fmt = nil
  for k, v in pairs(p:GetRenderFormats() or {}) do
    if tostring(v):lower() == "mkv" then fmt = k break end
  end
  if not fmt then return nil, "no_mkv" end
  if not p:SetCurrentRenderFormatAndCodec(fmt, "ProRes422HQ") then return nil, "fmt_codec_failed" end
  p:SetRenderSettings({ TargetDir = OUTDIR, SelectAllFrames = true })
  local jobId = p:AddRenderJob()
  if not jobId or jobId == "" then return nil, "add_job_failed" end
  return "job=" .. esc(jobId)
end

ops.render_start = function(args)
  local p = cur()
  local jobs = p:GetRenderJobList()
  if not jobs or #jobs == 0 then return nil, "no_jobs" end
  local ids = {}
  for _, j in ipairs(jobs) do ids[#ids + 1] = j["JobId"] end
  local ok = p:StartRendering(ids)
  if not ok then return nil, "start_failed" end
  return "jobs=" .. #ids
end

ops.render_status = function(args)
  local p = cur()
  local jobs = p:GetRenderJobList()
  if not jobs or #jobs == 0 then return nil, "no_jobs" end
  local j = jobs[#jobs]
  local st = p:GetRenderJobStatus(j["JobId"])
  return "job=" .. esc(j["JobId"])
    .. "|status=" .. esc(st and st["JobStatus"] or "?")
    .. "|pct=" .. tostring(st and st["CompletionPercentage"] or -1)
    .. "|err=" .. esc(st and st["Error"] or "-")
end

ops.set_marker = function(args)
  local p = cur()
  local tl = p:GetCurrentTimeline()
  if not tl then return nil, "no_timeline" end
  local frame = math.floor(tonumber(args.frame) or 0)
  if frame < 0 or frame > 1000000 then return nil, "frame_out_of_range" end
  local name = esc(args.name or "vb_marker")
  if #name > 80 then name = name:sub(1, 80) end
  local note = esc(args.note or "")
  if #note > 200 then note = note:sub(1, 200) end
  local ok = tl:AddMarker(frame, "Blue", name, note, 1)
  if not ok then return nil, "add_marker_failed" end
  return "frame=" .. frame
end

ops.get_state = function(args)
  local p = cur()
  if not p then return "no_project" end
  local tl = p:GetCurrentTimeline()
  if not tl then return "no_timeline" end
  return "tl=" .. esc(tl:GetName())
    .. "|start=" .. tostring(tl:GetStartFrame())
    .. "|end=" .. tostring(tl:GetEndFrame())
    .. "|vtracks=" .. tostring(tl:GetTrackCount("video"))
end

-- ---------- worker loop ----------
print("VBWORKER2|starting|ipc=" .. IPC)

if not wfile(BEAT, tostring(os.time())) then
  print("VBWORKER2|ERR|cannot_write|" .. IPC)
  return
end
print("VBWORKER2|live|beat_written")

-- 2ag: load the cover/edit extension ops INSIDE the worker (the polling
-- loop below blocks the Console thread, so no later paste can run).
local okExt, errExt = pcall(dofile, "/home/Alex/.local/share/DaVinciResolve/Fusion/Scripts/vb_cover_ops.lua")
if okExt then
  print("VBWORKER2|cover_ext=loaded")
else
  print("VBWORKER2|cover_ext_error=" .. tostring(errExt))
end

-- SELF-TEST: prove the read+parse pipeline before serving
do
  local probe = '{ "id" : 7, "op" : "status", "args" : {} }'
  local a = probe:match([=["id"%s*:%s*(%-?%d+)]=])
  local b = probe:match([=["op"%s*:%s*"(%w+)"]=])
  local c = probe:match([=["args"%s*:%s*(%{.*%})]=])
  print("VBSELF|id="..tostring(a).."|op="..tostring(b).."|args="..tostring(c))
  wfile(IPC .. "/selftest", "id="..tostring(a).."|op="..tostring(b))
end


local t0 = os.time()
local n = 0
while (os.time() - t0) < MAX_SECONDS do
  wfile(BEAT, tostring(os.time()))
  if exists(CMDF) then
    local raw = rfile(CMDF, 8192)
    local id = -1
    if raw then
      local m = raw:match([=["id"%s*:%s*(%-?%d+)]=])
      if m then id = tonumber(m) end
    end
    local op = raw and (raw:match([=["op"%s*:%s*"([%w_]+)"]=]) or raw:match("op=([%w_]+)"))
    local argstr = raw and raw:match([=["args"%s*:%s*(%{.*%})]=])
    local ok, data
    if op and ops[op] then
      local ran, r1, r2 = pcall(ops[op], parse_args(argstr))
      if ran then
        ok = (r1 ~= nil)
        data = r1 or r2 or ""
      else
        ok, data = false, "panic: " .. esc(r1)
      end
    elseif op then
      ok, data = false, "unknown_op:" .. esc(op)
    else
      ok, data = false, "malformed_cmd"
    end
    wfile(RESF, string.format('{"id":%d,"ok":%s,"data":"%s"}', id, tostring(ok), esc(data)))
    pcall(function() ffi.C.remove(CMDF) end)
    n = n + 1
    print("VBWORKER2|done|id=" .. id .. "|op=" .. esc(op) .. "|ok=" .. tostring(ok) .. "|n=" .. n)
  end
  bmd.wait(POLL)
end

print("VBWORKER2|stopped|max_time|n=" .. n)

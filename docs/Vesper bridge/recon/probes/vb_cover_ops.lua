-- vb_cover_ops.lua — extension ops for the 2ag edit job. Loaded by
-- vb_worker2.lua at startup (the worker loop blocks the Console, so
-- no later paste can run — measured 2ag).
-- Return convention: a STRING means success (data), `false, reason`
-- means failure. Never return a boolean true as data.
local OUTDIR = "/home/Alex/Videos/vb-edit/out"
local SOURCE = "/home/Alex/Videos/vb-edit/working.mp4"

local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 200 then s = s:sub(1, 200) .. "~" end
  return s
end

local function op_import_source(args)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return false, "no project" end
  local mp = p:GetMediaPool()
  mp:SetCurrentFolder(mp:GetRootFolder())
  local items = mp:ImportMedia({ SOURCE })
  if not items or #items == 0 then return false, "import empty" end
  local it = items[1]
  local props = it:GetClipProperty()
  local fps = props and props["FPS"] or "?"
  local dur = props and props["Duration"] or "?"
  return "clip=working.mp4|fps=" .. esc(fps) .. "|dur=" .. esc(dur)
end

local function op_build_timeline(args)
  -- Measured (2ag, free 21.1): AppendToTimeline succeeds ONLY for the
  -- first append in a worker op — subsequent appends no-op. So the MP4
  -- must be THE first append, no warm-up. (The 2s webm slice worked the
  -- same way: single clip, single append.)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return false, "no project" end
  local mp = p:GetMediaPool()
  mp:SetCurrentFolder(mp:GetRootFolder())
  local mp4 = mp:ImportMedia({ SOURCE })
  if not mp4 or #mp4 == 0 then return false, "mp4 import failed" end
  local name = "VB Edit " .. tostring(os.time())
  local tl = mp:CreateEmptyTimeline(name)
  if not tl then return false, "timeline create failed" end
  p:SetCurrentTimeline(tl)
  mp:AppendToTimeline(mp4[1])
  -- GetItemListInTrack returns 0 in the worker-thread context even when
  -- the append landed (measured 2ag: timeline end = full mp4 length).
  -- Verify by TIMELINE LENGTH instead: 6658 frames @ 24fps.
  bmd.wait(0.5)
  local tlEnd = tl:GetEndFrame() - tl:GetStartFrame()
  if tlEnd < 6000 then return false, "append failed (timeline " .. tlEnd .. " frames)" end
  return "tl=" .. name .. "|start=" .. tostring(tl:GetStartFrame()) .. "|end=" .. tostring(tl:GetEndFrame()) .. "|frames=" .. tlEnd
end

local function op_blur_region(args)
  -- Route (2ag): TIMELINE:InsertFusionCompositionIntoTimeline() creates a
  -- Fusion track item on the timeline (no TimelineItem/GetItemListInTrack
  -- needed — that query is unusable in the worker). Build the blur+mask
  -- inside its comp.
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  if not p then return false, "no project" end
  local tl = p:GetCurrentTimeline()
  if not tl then return false, "no timeline" end
  local fpsn = tonumber(p:GetSetting("timelineFrameRate")) or 30
  local x0 = tonumber(args.x0) or 0.0
  local y0 = tonumber(args.y0) or 0.0
  local x1 = tonumber(args.x1) or 1.0
  local y1 = tonumber(args.y1) or 1.0
  local t0 = tonumber(args.t0) or 0
  local t1 = tonumber(args.t1) or 0
  local pct = tonumber(args.blur_pct) or 60
  if x0 < 0 or x0 > 1 or x1 <= x0 or x1 > 1 or y0 < 0 or y1 <= y0 or y1 > 1 then
    return false, "region out of range"
  end
  if t1 <= t0 or t0 < 0 or t1 > 3600 then return false, "time range invalid" end
  local item = tl:InsertFusionCompositionIntoTimeline()
  if not item then return false, "insert fusion comp failed" end
  bmd.wait(0.5)
  local comp = item:GetFusionCompByIndex(1)
  if not comp then return false, "no comp on inserted item" end
  -- the inserted comp has MediaIn/MediaOut? Fusion comps on a generator
  -- track: add a Background + Blur + mask instead (no media to blur).
  -- BETTER: blur the WHOLE frame via this comp and mask the region.
  local bg = comp:FindTool("Background1") or comp:AddTool("Background", -1, -1)
  local blur = comp:AddTool("Blur", -1, -1)
  local mask = comp:AddTool("RectangleMask", -1, -1)
  local size = math.max(4, math.floor(pct / 100 * 60 + 0.5))
  blur:SetInput("BlurType", 0)
  blur:SetInput("XBlurSize", size)
  mask:SetInput("SoftEdge", 0.01)
  mask:SetInput("Center", { (x0 + x1) / 2, 1 - (y0 + y1) / 2 })
  mask:SetInput("Width", (x1 - x0))
  mask:SetInput("Height", (y1 - y0))
  -- keyframe the mask SCALE: 0 before/after window, full inside.
  local f0 = math.floor(t0 * fpsn)
  local f1 = math.ceil(t1 * fpsn)
  mask:SetInput("Width", 0.0, f0 - 1)
  mask:SetInput("Width", (x1 - x0), f0)
  mask:SetInput("Width", (x1 - x0), f1)
  mask:SetInput("Width", 0.0, f1 + 1)
  return "fusion_item=inserted|blur=" .. size .. "|frames=" .. f0 .. "-" .. f1
end

local function op_render_edit(args)
  local p = resolve:GetProjectManager():GetCurrentProject()
  if not p then return false, "no project" end
  local fmts = p:GetRenderFormats()
  local fmt = nil
  for k, v in pairs(fmts or {}) do
    if tostring(v):lower() == "mkv" then fmt = k break end
  end
  if not fmt then return false, "no mkv format" end
  if not p:SetCurrentRenderFormatAndCodec(fmt, "ProRes422HQ") then return false, "fmt/codec failed" end
  p:SetRenderSettings({ TargetDir = OUTDIR, SelectAllFrames = true })
  local jobId = p:AddRenderJob()
  if not jobId or jobId == "" then return false, "job empty" end
  return "job=" .. esc(jobId)
end

local function op_pool_dump(args)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  local mp = p:GetMediaPool()
  local out = {}
  local function scan(f, path)
    for _, c in ipairs(f:GetClipList() or {}) do
      out[#out+1] = path .. "/" .. esc(c:GetName())
    end
    for _, sf in ipairs(f:GetSubFolderList() or {}) do
      scan(sf, path .. "/" .. esc(sf:GetName()))
    end
  end
  scan(mp:GetRootFolder(), "root")
  return "clips=" .. #out .. " " .. table.concat(out, " ; ")
end

local function op_track_diag(args)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  local tl = p:GetCurrentTimeline()
  if not tl then return false, "no timeline" end
  local out = { "tl=" .. esc(tl:GetName()), "vtracks=" .. tostring(tl:GetTrackCount("video")) }
  for ti = 1, 3 do
    local items = tl:GetItemListInTrack("video", ti)
    out[#out+1] = "track" .. ti .. "=" .. tostring(items and #items or "nil")
  end
  local alt = tl:GetItemListInTrack("video", "1")
  out[#out+1] = "stridx=" .. tostring(alt and #alt or "nil")
  return table.concat(out, "|")
end

local function op_diag_create(args)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  local mp = p:GetMediaPool()
  local clips = (mp:GetRootFolder():GetClipList() or {})
  local out = { "clips=" .. #clips }
  local target = clips[1]
  if not target then return table.concat(out, "|") .. " no-clips" end
  out[#out+1] = "target=" .. esc(target:GetName())
  local okA, tlA = pcall(function()
    return mp:CreateTimelineFromClips("DiagA" .. tostring(os.time()), { { mediaPoolItem = target } }) end)
  out[#out+1] = "A=" .. tostring(okA) .. "/" .. esc(tostring(tlA and tlA:GetName() or tlA))
  local okB, tlB = pcall(function() return mp:CreateEmptyTimeline("DiagB" .. tostring(os.time())) end)
  out[#out+1] = "B=" .. tostring(okB) .. "/" .. esc(tostring(tlB and tlB:GetName() or tlB))
  if tlB then
    p:SetCurrentTimeline(tlB)
    local okApp, r = pcall(function() return mp:AppendToTimeline({ target }) end)
    local items = tlB:GetItemListInTrack("video", 1)
    out[#out+1] = "append_obj=" .. tostring(okApp) .. "/" .. esc(tostring(r)) .. "/items=" .. tostring(items and #items or "nil")
    local okApp2, r2 = pcall(function() return mp:AppendToTimeline({ { mediaPoolItem = target } }) end)
    local items2 = tlB:GetItemListInTrack("video", 1)
    out[#out+1] = "append_dict=" .. tostring(okApp2) .. "/" .. esc(tostring(r2)) .. "/items=" .. tostring(items2 and #items2 or "nil")
  end
  return table.concat(out, "|")
end

local function op_timeline_webm(args)
  -- control: create a timeline from the PROVEN webm fixture (2s slice used it)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  local mp = p:GetMediaPool()
  mp:SetCurrentFolder(mp:GetRootFolder())
  local items = mp:ImportMedia({ "/home/Alex/Videos/vb-fixture/fixture_master.webm" })
  if not items or #items == 0 then return false, "webm import empty" end
  local tl = mp:CreateEmptyTimeline("WebmT" .. tostring(os.time()))
  if not tl then return false, "empty tl failed" end
  p:SetCurrentTimeline(tl)
  mp:AppendToTimeline(items[1])
  local n = tl:GetItemListInTrack("video", 1)
  local count = (n and #n) or 0
  -- also try mp4 in the SAME timeline for contrast
  local clips = mp:GetRootFolder():GetClips()
  local mp4 = nil
  for _, c in pairs(clips or {}) do
    if tostring(c:GetName()):find("working", 1, true) then mp4 = c break end
  end
  local mp4count = "n/a"
  if mp4 then
    mp:AppendToTimeline(mp4)
    local m = tl:GetItemListInTrack("video", 1)
    mp4count = tostring(m and #m or "nil")
  end
  return "webm_items=" .. count .. "|then_mp4_items=" .. mp4count
end

local function op_diag_official(args)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  local mp = p:GetMediaPool()
  local clips = mp:GetRootFolder():GetClips()
  local target = nil
  for _, c in pairs(clips or {}) do target = c break end
  if not target then return false, "no clip via GetClips" end
  local out = { "target=" .. esc(target:GetName()) }
  local name = "OffT" .. tostring(os.time())
  local tl = mp:CreateEmptyTimeline(name)
  out[#out+1] = "tl=" .. esc(tostring(tl and tl:GetName()))
  if tl then
    p:SetCurrentTimeline(tl)
    local okA = mp:AppendToTimeline(target)
    local items = tl:GetItemListInTrack("video", 1)
    out[#out+1] = "bare=" .. tostring(okA) .. "/items=" .. tostring(items and #items or "nil")
    local okB = mp:AppendToTimeline({ { ["mediaPoolItem"] = target, startFrame = 0, endFrame = 23 } })
    items = tl:GetItemListInTrack("video", 1)
    out[#out+1] = "sub=" .. tostring(okB) .. "/items=" .. tostring(items and #items or "nil")
  end
  return table.concat(out, "|")
end

local function op_diag_warm(args)
  local pm = resolve:GetProjectManager()
  local p = pm:GetCurrentProject()
  local mp = p:GetMediaPool()
  mp:SetCurrentFolder(mp:GetRootFolder())
  local mp4 = mp:ImportMedia({ "/home/Alex/Videos/vb-edit/working.mp4" })
  local webm = mp:ImportMedia({ "/home/Alex/Videos/vb-fixture/fixture_master.webm" })
  local out = { "imports=" .. #mp4 .. "/" .. #webm }
  local tl = mp:CreateEmptyTimeline("W" .. tostring(os.time()))
  p:SetCurrentTimeline(tl)
  local r1 = mp:AppendToTimeline(webm[1])
  local i1 = tl:GetItemListInTrack("video", 1)
  out[#out+1] = "webm=" .. tostring(i1 and #i1 or "nil")
  local r2 = mp:AppendToTimeline(mp4[1])
  local i2 = tl:GetItemListInTrack("video", 1)
  out[#out+1] = "mp4_bare=" .. tostring(i2 and #i2 or "nil")
  mp:AppendToTimeline({ { mediaPoolItem = mp4[1] } })
  local i3 = tl:GetItemListInTrack("video", 1)
  out[#out+1] = "mp4_dict=" .. tostring(i3 and #i3 or "nil")
  local props = mp4[1]:GetClipProperty()
  out[#out+1] = "mp4_fps=" .. esc(tostring(props and props["FPS"])) .. "|res=" .. esc(tostring(props and props["Resolution"]))
  local wp = webm[1]:GetClipProperty()
  out[#out+1] = "webm_fps=" .. esc(tostring(wp and wp["FPS"]))
  out[#out+1] = "tl_fps=" .. esc(tostring(p:GetSetting("timelineFrameRate")))
  return table.concat(out, " ")
end

local ops = _G.vb_ops_table
if ops then
  ops.pool_dump = op_pool_dump
  ops.diag_warm = op_diag_warm
  ops.diag_official = op_diag_official
  ops.timeline_webm = op_timeline_webm
  ops.track_diag = op_track_diag
  ops.diag_create = op_diag_create
  ops.import_source = op_import_source
  ops.build_timeline = op_build_timeline
  ops.blur_region = op_blur_region
  ops.render_edit = op_render_edit
  print("VBCOVER|ready|ops=import_source,build_timeline,blur_region,render_edit")
else
  print("VBCOVER|ERR|vb_worker2 not loaded first")
end

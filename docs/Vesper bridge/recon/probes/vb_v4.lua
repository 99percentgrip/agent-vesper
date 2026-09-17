-- Vesper Bridge bootstrap v4: marker-based request/response channel probe
local function esc(s)
  return tostring(s or "?"):gsub("[%|\n\r]", " ")
end
local pm = resolve:GetProjectManager()
local proj = pm:GetCurrentProject()
if not proj then
  print("VB4|ERR|no-project")
  return
end
local tl = proj:GetCurrentTimeline()
if not tl then
  tl = proj:GetMediaPool():CreateEmptyTimeline("vb_channel")
  if not tl then
    print("VB4|ERR|no-timeline-cannot-create")
    return
  end
  proj:SetCurrentTimeline(tl)
end
local ok = tl:AddMarker(0, "Blue", "vb_hello", "vesper-bridge", 1, "VB4|cap|21.1.0.17|free")
print("VB4|marker_set=" .. tostring(ok) .. "|tl=" .. esc(tl:GetName()) .. "|proj=" .. esc(proj:GetName()))
local cd = tl:GetMarkerCustomData(0)
print("VB4|readback=" .. esc(cd))

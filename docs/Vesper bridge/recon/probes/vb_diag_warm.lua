-- vb_diag_warm.lua — isolate the mp4-append failure precisely (2ag)
local function esc(s) s = tostring(s or "?") s = s:gsub("[%|\r\n\"%c]", " ") return s end
local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
local mp = p:GetMediaPool()
mp:SetCurrentFolder(mp:GetRootFolder())
local mp4 = mp:ImportMedia({ "/home/Alex/Videos/vb-edit/working.mp4" })
local webm = mp:ImportMedia({ "/home/Alex/Videos/vb-fixture/fixture_master.webm" })
print("VBOK|warm|mp4=" .. #mp4 .. " webm=" .. #webm)
local tl = mp:CreateEmptyTimeline("W" .. tostring(os.time()))
p:SetCurrentTimeline(tl)
local r1 = mp:AppendToTimeline(webm[1])
local i1 = tl:GetItemListInTrack("video", 1)
print("VBOK|warm|webm_append=" .. esc(tostring(r1)) .. " items=" .. tostring(i1 and #i1))
local r2 = mp:AppendToTimeline(mp4[1])
local i2 = tl:GetItemListInTrack("video", 1)
print("VBOK|warm|mp4_append=" .. esc(tostring(r2)) .. " items=" .. tostring(i2 and #i2))
-- try dict form for mp4 now
local r3 = mp:AppendToTimeline({ { mediaPoolItem = mp4[1] } })
local i3 = tl:GetItemListInTrack("video", 1)
print("VBOK|warm|mp4_dict=" .. esc(tostring(r3)) .. " items=" .. tostring(i3 and #i3))
-- clip props that might explain: codec/resolution mismatch vs timeline settings
local props = mp4[1]:GetClipProperty()
local keep = {}
for _, k in ipairs({ "Format", "Resolution", "FPS", "Video Codec", "File Name" }) do
  keep[#keep+1] = k .. "=" .. esc(tostring(props and props[k]))
end
print("VBOK|warm|mp4_props " .. table.concat(keep, " "))
local wp = webm[1]:GetClipProperty()
local wk = {}
for _, k in ipairs({ "Format", "Resolution", "FPS", "Video Codec" }) do
  wk[#wk+1] = k .. "=" .. esc(tostring(wp and wp[k]))
end
print("VBOK|warm|webm_props " .. table.concat(wk, " "))
print("VBOK|warm|tl_settings fps=" .. esc(tostring(p:GetSetting("timelineFrameRate"))) ..
      " res=" .. esc(tostring(p:GetSetting("timelineResolutionWidth"))) .. "x" .. esc(tostring(p:GetSetting("timelineResolutionHeight"))))

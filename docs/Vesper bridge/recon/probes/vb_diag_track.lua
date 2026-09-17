-- vb_diag_track.lua — why does a timeline with vtracks=1 report no items? (2ag)
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  return s
end
local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
local tl = p:GetCurrentTimeline()
if not tl then print("VBOK|diag|no timeline") return end
print("VBOK|diag|tl=" .. esc(tl:GetName()))
print("VBOK|diag|trackcount_video=" .. tostring(tl:GetTrackCount("video")))
for ti = 1, 3 do
  local items = tl:GetItemListInTrack("video", ti)
  print("VBOK|diag|track" .. ti .. "_items=" .. tostring(items and #items or "nil"))
end
-- try ApplyGradeFrom... no: check GetTrackName + the alternative API spellings
print("VBOK|diag|trackname1=" .. esc(tl:GetTrackName("video", 1)))
-- maybe items come from tl:GetItemListInTrack with string index?
local alt = tl:GetItemListInTrack("video", "1")
print("VBOK|diag|string_index_items=" .. tostring(alt and #alt or "nil"))

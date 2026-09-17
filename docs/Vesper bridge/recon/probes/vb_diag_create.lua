-- vb_diag_create.lua — why do BOTH timeline-creation routes fail? (2ag)
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  return s
end
local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
local mp = p:GetMediaPool()
local root = mp:GetRootFolder()
local clips = root:GetClipList() or {}
print("VBOK|diag|root_clips=" .. #clips)
local target = clips[1]
if not target then print("VBOK|diag|no clips at all") return end
print("VBOK|diag|target=" .. esc(target:GetName()))
-- route A: dict form
local okA, tlA = pcall(function()
  return mp:CreateTimelineFromClips("Diag A " .. tostring(os.time()), { { mediaPoolItem = target } })
end)
print("VBOK|diag|A_dict=" .. tostring(okA) .. "|" .. esc(tostring(tlA and tlA:GetName() or tlA)))
-- route A2: table with the object under mediaPoolItem key, string name variant
local okA2, tlA2 = pcall(function()
  return mp:CreateTimelineFromClips("DiagA2", { { ["mediaPoolItem"] = target } })
end)
print("VBOK|diag|A2=" .. tostring(okA2) .. "|" .. esc(tostring(tlA2)))
-- route B: empty + append
local okB, tlB = pcall(function() return mp:CreateEmptyTimeline("Diag B") end)
print("VBOK|diag|B_empty=" .. tostring(okB) .. "|" .. esc(tostring(tlB)))
if tlB then
  p:SetCurrentTimeline(tlB)
  local okApp, r = pcall(function() return mp:AppendToTimeline({ target }) end)
  print("VBOK|diag|B_append=" .. tostring(okApp) .. "|" .. esc(tostring(r)))
  local items = tlB:GetItemListInTrack("video", 1)
  print("VBOK|diag|B_items=" .. tostring(items and #items or "nil"))
end
-- route C: AppendToTimeline with dict form
if tlB then
  local okC, rC = pcall(function()
    return mp:AppendToTimeline({ { mediaPoolItem = target } })
  end)
  print("VBOK|diag|C_append_dict=" .. tostring(okC) .. "|" .. esc(tostring(rC)))
  local itemsC = tlB:GetItemListInTrack("video", 1)
  print("VBOK|diag|C_items=" .. tostring(itemsC and #itemsC or "nil"))
end

-- vb_diag_official.lua — run the OFFICIAL example EXACTLY as shipped, on our clip (2ag)
-- If the shipped pattern itself fails on this build/edition, that is the answer.
local function esc(s) s = tostring(s or "?") s = s:gsub("[%|\r\n\"%c]", " ") return s end

local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
local mp = p:GetMediaPool()
local root = mp:GetRootFolder()
-- example uses GetClips() (map), not GetClipList()
local clips = root:GetClips()
print("VBOK|off|GetClips count=" .. (clips and "table" or tostring(clips)))
local target = nil
for k, c in pairs(clips or {}) do
  print("VBOK|off|clip " .. esc(tostring(k)) .. " = " .. esc(c and c:GetName()))
  if not target then target = c end
end
if not target then print("VBOK|off|NO TARGET") return end
-- official: empty timeline + append bare clip
local name = "OffT" .. tostring(os.time())
local tl = mp:CreateEmptyTimeline(name)
print("VBOK|off|empty_tl=" .. esc(tostring(tl)))
if not tl then return end
p:SetCurrentTimeline(tl)
local okA = mp:AppendToTimeline(target)
print("VBOK|off|append_bare=" .. esc(tostring(okA)))
local items = tl:GetItemListInTrack("video", 1)
print("VBOK|off|items=" .. tostring(items and #items or "nil"))
-- variant: subClip table as example 7
local okB = mp:AppendToTimeline({ { ["mediaPoolItem"] = target, startFrame = 0, endFrame = 23 } })
items = tl:GetItemListInTrack("video", 1)
print("VBOK|off|subclip_append=" .. esc(tostring(okB)) .. " items=" .. tostring(items and #items or "nil"))

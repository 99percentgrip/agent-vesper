-- vb_diag_pool.lua — where did the imported clip land? (2ag)
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  return s
end
local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
local mp = p:GetMediaPool()
local root = mp:GetRootFolder()
print("VBOK|diag|root_clips=" .. #(root:GetClipList() or {}))
for _, c in ipairs(root:GetClipList() or {}) do
  print("VBOK|diag|root: " .. esc(c:GetName()))
end
for i, f in ipairs(root:GetSubFolderList() or {}) do
  print("VBOK|diag|subfolder " .. i .. ": " .. esc(f:GetName()) .. " clips=" .. #(f:GetClipList() or {}))
  for _, c in ipairs(f:GetClipList() or {}) do
    print("VBOK|diag|  in-sub: " .. esc(c:GetName()))
  end
  if i >= 5 then break end
end
-- also: current folder
local cur = mp:GetCurrentFolder()
print("VBOK|diag|current_folder=" .. esc(cur and cur:GetName()))

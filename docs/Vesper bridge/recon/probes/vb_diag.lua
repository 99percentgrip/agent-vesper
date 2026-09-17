-- Vesper Bridge diag: why did ImportMedia return empty for the .webm fixture?
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 200 then s = s:sub(1, 200) .. "~" end
  return s
end

local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
if not p then print("VBERR|diag|no project") return end

-- 1. MediaStorage: what does Resolve SEE on disk?
local ms = resolve:GetMediaStorage()
if ms then
  local mounted = ms:GetMountedVolumeList()
  local n = 0
  local parts = {}
  if type(mounted) == "table" then
    for _, v in ipairs(mounted) do
      n = n + 1
      if n <= 5 then parts[#parts + 1] = esc(v) end
    end
  end
  print("VBOK|diag|volumes=" .. n .. "|" .. table.concat(parts, ","))
  local fileList = ms:GetFileList("/home/Alex/Videos/vb-fixture")
  if type(fileList) == "table" then
    local c = 0
    local fl = {}
    for _, f in ipairs(fileList) do
      c = c + 1
      if c <= 5 then fl[#fl + 1] = esc(f) end
    end
    print("VBOK|diag|filelist_count=" .. c .. "|" .. table.concat(fl, ","))
  else
    print("VBERR|diag|filelist=" .. esc(fileList))
  end
else
  print("VBERR|diag|no media storage")
end

-- 2. Does the OLD ImportMedia string form exist on this build?
local mp = p:GetMediaPool()
if not mp then print("VBERR|diag|no media pool") return end

-- 3. Try ImportMedia with plain string list (legacy form) into root
local root = mp:GetRootFolder()
mp:SetCurrentFolder(root)
local ok, r1 = pcall(function() return mp:ImportMedia({ "/home/Alex/Videos/vb-fixture/fixture_master.webm" }) end)
print("VBOK|diag|string_form=" .. tostring(ok) .. "|result_type=" .. type(r1))
if type(r1) == "table" then
  print("VBOK|diag|string_form_count=" .. #r1)
end

-- 4. Try dict form once more but capture the actual return type
local ok2, r2 = pcall(function() return mp:ImportMedia({ { FilePath = "/home/Alex/Videos/vb-fixture/fixture_master.webm" } }) end)
print("VBOK|diag|dict_form=" .. tostring(ok2) .. "|result_type=" .. type(r2))
if type(r2) == "table" then
  print("VBOK|diag|dict_form_count=" .. #r2)
end

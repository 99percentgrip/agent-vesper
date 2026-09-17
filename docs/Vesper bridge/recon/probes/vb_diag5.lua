-- vb_diag5: WHAT is actually available in the Console Lua sandbox?
-- Goal: find file I/O (bmd.readfile/writefile) + timing (bmd.wait/os.time/os.clock)
-- to decide if a paste-once-per-launch persistent worker is possible.
local names = {}
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 160 then s = s:sub(1, 160) .. "~" end
  return s
end

print("VBOK|diag5|globals=" .. esc(table.concat({
  "bmd=" .. tostring(type(bmd)),
  "os=" .. tostring(type(os)),
  "io=" .. tostring(type(io)),
  "require=" .. tostring(type(require)),
  "socket=" .. tostring(type(socket)),
}, ",")))

if type(bmd) == "table" then
  local fns = {}
  for k, v in pairs(bmd) do
    if type(v) == "function" then fns[#fns + 1] = k end
  end
  table.sort(fns)
  print("VBOK|diag5|bmd_fns=" .. esc(table.concat(fns, ",")))
end

if type(os) == "table" then
  local osf = {}
  for k, v in pairs(os) do
    if type(v) == "function" then osf[#osf + 1] = k end
  end
  table.sort(osf)
  print("VBOK|diag5|os_fns=" .. esc(table.concat(osf, ",")))
end

-- direct probes (pcall each)
local probes = {
  { "bmd.readfile", function() return bmd and bmd.readfile and bmd.readfile("/etc/hostname") end },
  { "bmd.writefile", function() return bmd and bmd.writefile and bmd.writefile("/tmp/vb_w.txt", "x") end },
  { "bmd.wait", function() return bmd and bmd.wait and bmd.wait(0) end },
  { "bmd.scriptlib", function() return bmd and bmd.scriptlib and "present" end },
  { "os.time", function() return os and os.time and os.time() end },
  { "os.clock", function() return os and os.clock and os.clock() end },
}
for _, pr in ipairs(probes) do
  local ok, r = pcall(pr[2])
  print("VBOK|diag5|" .. pr[1] .. "=" .. tostring(ok) .. "|" .. esc(r))
end

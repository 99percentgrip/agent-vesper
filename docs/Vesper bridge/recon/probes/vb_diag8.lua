-- vb_diag8: probe bmd.writestring / bmd.readstring signatures (the REAL file I/O).
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 240 then s = s:sub(1, 240) .. "~" end
  return s
end

-- candidate forms
local forms = {
  { "w_str_str", function() return bmd.writestring("/home/Alex/Videos/vb-fixture/ipc/t1.txt", "alpha") end },
  { "w_str_num", function() return bmd.writestring("/home/Alex/Videos/vb-fixture/ipc/t2.txt", 42) end },
  { "w_tbl", function() return bmd.writestring("/home/Alex/Videos/vb-fixture/ipc/t3.txt", { a = 1 }) end },
}
for _, f in ipairs(forms) do
  local ok, r = pcall(f[2])
  print("VBOK|diag8|" .. f[1] .. "=" .. tostring(ok) .. "|ret=" .. esc(r))
end

-- read back: string path
local r1 = pcall(function() return bmd.readstring("/home/Alex/Videos/vb-fixture/ipc/t1.txt") end)
local v1ok, v1 = r1
print("VBOK|diag8|r_path=" .. tostring(v1ok) .. "|type=" .. type(v1) .. "|val=" .. esc(v1))

-- maybe readstring takes content not path?
local r2 = pcall(function() return bmd.readstring("alpha") end)
print("VBOK|diag8|r_content=" .. tostring(r2))

-- wait + re-verify write stickiness
bmd.wait(0.2)
local w4 = pcall(function() return bmd.writestring("/home/Alex/Videos/vb-fixture/ipc/t4.txt", "beta") end)
print("VBOK|diag8|w_after_wait=" .. tostring(w4))

-- fileexists sanity
local fe = pcall(function() return bmd.fileexists("/home/Alex/Videos/vb-fixture/ipc/t1.txt") end)
local feok, fev = fe
print("VBOK|diag8|fileexists_t1=" .. tostring(fev))

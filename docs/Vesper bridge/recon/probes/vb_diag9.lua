-- vb_diag9: settle writefile vs writestring with a STRICT existence check.
-- The beat file at 14:03:28 proves SOME form writes to the real FS.
-- That call was: bmd.writefile("/path/beat", "1789538608")  ← via pcall
-- diag8 used bmd.writestring — which returned a path but wrote nothing.
-- Test: does bmd.writefile exist as a REAL function here?
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 240 then s = s:sub(1, 240) .. "~" end
  return s
end

print("VBOK|diag9|type_bmd=" .. type(bmd))
print("VBOK|diag9|writefile_type=" .. tostring(bmd and bmd.writefile))
print("VBOK|diag9|writestring_type=" .. tostring(bmd and bmd.writestring))

-- writefile probe with existence verification VIA THE SAME API
if bmd and bmd.writefile then
  local ok, r = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/d9a.txt", "written_by_writefile") end)
  print("VBOK|diag9|writefile_call=" .. tostring(ok) .. "|ret=" .. esc(r))
end

if bmd and bmd.fileexists then
  local fe = bmd.fileexists("/home/Alex/Videos/vb-fixture/ipc/d9a.txt")
  print("VBOK|diag9|fileexists_d9a=" .. esc(fe))
end

-- writestring with a relative name (maybe it maps into a virtual root)
local ok2, r2 = pcall(function() return bmd.writestring("d9b.txt", "via_writestring_rel") end)
print("VBOK|diag9|writestring_rel=" .. tostring(ok2) .. "|ret=" .. esc(r2))

-- THE CONTROL: repeat the EXACT worker beat call
local ok3, r3 = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/beat2", tostring(os.time())) end)
print("VBOK|diag9|beat2_call=" .. tostring(ok3) .. "|ret=" .. esc(r3))

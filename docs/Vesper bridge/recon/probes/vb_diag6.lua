-- vb_diag6: measure EXACTLY how bmd.writefile behaves in Console Lua.
-- Worker failed twice at beat-write despite dir existing. Hypotheses:
--  H1: writefile(path, string) writes QUOTED text (beat contained "1789538608" with quotes)
--  H2: writefile rejects .beat / extensionless files
--  H3: writefile requires table content
--  H4: pcall around writefile swallows a real error we need to see
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 160 then s = s:sub(1, 160) .. "~" end
  return s
end

print("VBOK|diag6|probe each form, unswallowed:")

-- form A: string to extensionless file
local okA, rA = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/beatA", "123") end)
print("VBOK|diag6|A_string_noext=" .. tostring(okA) .. "|ret=" .. esc(rA))

-- form B: string to .txt
local okB, rB = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/beatB.txt", "123") end)
print("VBOK|diag6|B_string_txt=" .. tostring(okB) .. "|ret=" .. esc(rB))

-- form C: table to .json
local okC, rC = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/cmdC.json", { id = 1, op = "x" }) end)
print("VBOK|diag6|C_table_json=" .. tostring(okC) .. "|ret=" .. esc(rC))

-- form D: string to .json
local okD, rD = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/cmdD.json", "hello") end)
print("VBOK|diag6|D_string_json=" .. tostring(okD) .. "|ret=" .. esc(rD))

-- readback each
for _, n in ipairs({ "beatA", "beatB.txt", "cmdC.json", "cmdD.json" }) do
  local ok, v = pcall(function() return bmd.readfile("/home/Alex/Videos/vb-fixture/ipc/" .. n) end)
  print("VBOK|diag6|read_" .. n .. "=" .. tostring(ok) .. "|type=" .. type(v) .. "|val=" .. esc(v))
end

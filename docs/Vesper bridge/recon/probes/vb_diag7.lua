-- vb_diag7: FULL bmd function inventory (chunked, no truncation) + alternative read paths.
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 240 then s = s:sub(1, 240) .. "~" end
  return s
end

if type(bmd) ~= "table" then
  print("VBERR|diag7|no_bmd_table")
  return
end

local fns = {}
for k, v in pairs(bmd) do
  fns[#fns + 1] = k .. "(" .. type(v) .. ")"
end
table.sort(fns)

-- chunk the list so nothing is truncated
local CHUNK = 6
for i = 1, #fns, CHUNK do
  local part = {}
  for j = i, math.min(i + CHUNK - 1, #fns) do part[#part + 1] = fns[j] end
  print("VBOK|diag7|bmd_" .. i .. "=" .. esc(table.concat(part, ",")))
end

-- non-function members (values)
local vals = {}
for k, v in pairs(bmd) do
  if type(v) ~= "function" then vals[#vals + 1] = k .. "=" .. esc(v) end
end
if #vals > 0 then
  table.sort(vals)
  print("VBOK|diag7|bmd_values=" .. esc(table.concat(vals, ",")))
end

-- alternative read mechanisms
local alts = {
  { "bmd.scriptlib", function() return type(bmd.scriptlib) end },
  { "fusion.LoadScript", function() return fusion and type(fusion.LoadScript) end },
  { "fusion.GetResolvePath", function() return fusion and fusion.GetResolvePath and "fn" or "nil" end },
  { "print_buf", function() return type(print) end },
}
for _, a in ipairs(alts) do
  local ok, r = pcall(a[2])
  print("VBOK|diag7|" .. a[1] .. "=" .. tostring(ok) .. "|" .. esc(r))
end

-- does writefile overwrite? critical for ipc
local w1 = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/ow.txt", "one") end)
local w2 = pcall(function() return bmd.writefile("/home/Alex/Videos/vb-fixture/ipc/ow.txt", "two") end)
print("VBOK|diag7|overwrite=" .. tostring(w1) .. "," .. tostring(w2))

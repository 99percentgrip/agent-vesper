local f, e = loadfile("/home/Alex/.local/share/DaVinciResolve/Fusion/Scripts/vb_worker2.lua")
if f then
  print("SYNTAX_OK")
else
  print("SYNTAX_ERR: " .. tostring(e))
end

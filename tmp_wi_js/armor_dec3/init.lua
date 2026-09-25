function table.find(tbl, value)
    for i = 1, #tbl do
        if tbl[i] == value then
            return i
        end
    end
    return nil
end

runOnChange(function() platformCode = settings:getString("db.platformCode") end, "db.platformCode")

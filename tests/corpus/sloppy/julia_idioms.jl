module SloppyJulia

function checks(xs, x)
    a = length(xs) == 0
    for i in 1:length(xs)
        println(xs[i])
    end
    c = x == nothing
    return (a, c)
end

end

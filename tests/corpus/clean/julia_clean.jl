module CleanJulia

using LinearAlgebra
using Statistics

struct Point
    x::Float64
    y::Float64
end

struct Size
    x::Float64
    y::Float64
end

function checks(xs, x)
    a = isempty(xs)
    b = eachindex(xs)
    c = isnothing(x)
    return (a, b, c)
end

end


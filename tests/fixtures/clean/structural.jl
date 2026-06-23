module CleanFixture

export CreateUser, UpdateUser, normalize_user

struct CreateUser
    id::String
    name::String
    email::String
end

struct UpdateUser
    id::String
    name::String
    email::String
end

const CREATE_SCHEMA = (
    id = String,
    name = String,
    email = String,
)

const UPDATE_SCHEMA = (
    id = String,
    name = String,
    email = String,
)

function normalize_user(user)
    strip(user.name)
end

end

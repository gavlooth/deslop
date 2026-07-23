function require_save_activity(activity)
    observations = activity.controller_lambda_observations
    lambda_mean = activity.controller_lambda_mean
    input_mean = activity.input_effect_mean
    observations > 0 || throw(ArgumentError("save activity has no observations"))
    isfinite(lambda_mean) && lambda_mean > 0 ||
        throw(ArgumentError("save lambda mean is invalid"))
    isfinite(input_mean) && input_mean > 0 ||
        throw(ArgumentError("save input mean is invalid"))
    return true
end

function require_resume_activity(activity)
    observations = activity.controller_lambda_observations
    lambda_mean = activity.controller_lambda_mean
    input_mean = activity.input_effect_mean
    isnan(lambda_mean) && iszero(observations) && return true
    observations > 0 || throw(ArgumentError("resume activity has no observations"))
    isfinite(lambda_mean) && lambda_mean > 0 ||
        throw(ArgumentError("resume lambda mean is invalid"))
    isfinite(input_mean) && input_mean > 0 ||
        throw(ArgumentError("resume input mean is invalid"))
    return true
end

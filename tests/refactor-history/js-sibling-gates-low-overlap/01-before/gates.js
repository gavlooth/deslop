function requireSaveActivity(activity) {
  if (!(activity.controller_lambda_observations > 0)) {
    throw new Error("save activity has no observations");
  }
}

function verifyReleaseIdentity(status) {
  if (!(status.controller_lambda_observations === 0 && status.run_id === null)) {
    throw new Error("release identity is invalid");
  }
}

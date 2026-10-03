Keep it simple
Keep it lean
Don't code defensive, if something is not like we expect, we should crash or throw
Keep functions under 5 lines inline if they are only used once
Only implment what I asked, not more
Keep git diffs small if possible
Try to solve the problem in as little code as possible while staying readable
Give semantic names, no one letter variables
Only document in comments what is impossible to understand by reading the code
Keep comments brief, and usually don't comment at all
run cargo fmt && cargo check
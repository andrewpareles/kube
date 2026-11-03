export const removeFnFromArray = <FnType>(fns: FnType[], fn: FnType) => {
  for (let i = 0; i < fns.length; i += 1) {
    if (fns[i] !== fn) {
      continue;
    }
    fns.splice(i, 1);
    return;
  }
};
